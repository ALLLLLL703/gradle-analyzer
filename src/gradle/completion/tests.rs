//! Unit tests for the static-first completion engine (both DSLs).
//!
//! These prove the four layers independently: context classification per block, the `libs.`
//! catalog-accessor site, text-state suppression (comment / string / opaque → EMPTY), DSL
//! parity, and that ranking is a SEPARATE deterministic pass from eligibility.

use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{SemanticInput, analyze_documents};
use crate::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
use crate::i18n::Translator;
use tower_lsp::lsp_types::Url;

use super::{Candidate, CandidateKind, CompletionServices, complete};

/// The catalog fixture both DSL parity tests resolve `libs.*` accessors against.
const CATALOG: &str = r#"
[versions]
guava = "33.0.0-jre"
[libraries]
guava = { module = "com.google.guava:guava", version.ref = "guava" }
commons-lang3 = "org.apache.commons:commons-lang3:3.14.0"
[bundles]
networking = ["guava"]
[plugins]
spotless = "com.diffplug.spotless:6.25.0"
"#;

fn translator() -> Translator {
    Translator::new()
}

fn doc(text: &str, lang: DslLanguage) -> TrackedDocument {
    let (name, kind) = match lang {
        DslLanguage::Kotlin => ("build.gradle.kts", GradleFileKind::RootBuildScript(DslLanguage::Kotlin)),
        DslLanguage::Groovy => ("build.gradle", GradleFileKind::RootBuildScript(DslLanguage::Groovy)),
    };
    let uri = Url::from_file_path(format!("/proj/{name}")).unwrap();
    TrackedDocument::new(uri, 1, text, kind)
}

fn graph_with_catalog(
    build_id: &str,
    build_text: &str,
    build_kind: GradleFileKind,
) -> crate::gradle::semantic::SemanticGraph {
    analyze_documents(&[
        SemanticInput::script("gradle/libs.versions.toml", CATALOG, GradleFileKind::VersionCatalog),
        SemanticInput::script(build_id, build_text, build_kind),
    ])
}

fn run(text: &str, lang: DslLanguage, offset: usize) -> Vec<Candidate> {
    let document = doc(text, lang);
    let parse = match lang {
        DslLanguage::Kotlin => parse_kotlin(text),
        DslLanguage::Groovy => parse_groovy(text),
    };
    let tr = translator();
    let services = CompletionServices::new(&tr, 50);
    let graph = analyze_documents(&[]);
    complete(&document, &parse, &graph, offset, &services)
}

fn run_with_catalog(text: &str, lang: DslLanguage, offset: usize) -> Vec<Candidate> {
    let document = doc(text, lang);
    let parse = match lang {
        DslLanguage::Kotlin => parse_kotlin(text),
        DslLanguage::Groovy => parse_groovy(text),
    };
    let (id, kind) = match lang {
        DslLanguage::Kotlin => ("build.gradle.kts", GradleFileKind::RootBuildScript(DslLanguage::Kotlin)),
        DslLanguage::Groovy => ("build.gradle", GradleFileKind::RootBuildScript(DslLanguage::Groovy)),
    };
    let graph = graph_with_catalog(id, text, kind);
    let tr = translator();
    let services = CompletionServices::new(&tr, 50);
    complete(&document, &parse, &graph, offset, &services)
}

fn labels(candidates: &[Candidate]) -> Vec<&str> {
    candidates.iter().map(|c| c.label.as_str()).collect()
}

fn has_label(candidates: &[Candidate], label: &str) -> bool {
    candidates.iter().any(|c| c.label == label)
}

#[test]
fn top_level_offers_block_keywords_kotlin() {
    let text = "\n";
    let items = run(text, DslLanguage::Kotlin, 0);
    assert!(has_label(&items, "plugins"), "got {:?}", labels(&items));
    assert!(has_label(&items, "dependencies"));
    assert!(items.iter().all(|c| c.kind == CandidateKind::BlockKeyword));
}

#[test]
fn dependencies_block_offers_configurations_and_scaffold_groovy() {
    let text = "dependencies {\n    \n}\n";
    let offset = text.find("\n    \n").unwrap() + 5;
    let items = run(text, DslLanguage::Groovy, offset);
    assert!(has_label(&items, "implementation"), "got {:?}", labels(&items));
    assert!(has_label(&items, "testImplementation"));
    assert!(
        items.iter().any(|c| c.kind == CandidateKind::CoordinateScaffold),
        "a coordinate scaffold is offered: {:?}",
        labels(&items)
    );
}

#[test]
fn dependencies_block_offers_catalog_accessors_kotlin() {
    let text = "dependencies {\n    \n}\n";
    let offset = text.find("\n    \n").unwrap() + 5;
    let items = run_with_catalog(text, DslLanguage::Kotlin, offset);
    assert!(has_label(&items, "libs.guava"), "got {:?}", labels(&items));
    assert!(items.iter().any(|c| c.kind == CandidateKind::CatalogAccessor));
}

#[test]
fn plugins_block_offers_plugin_ids_kotlin() {
    let text = "plugins {\n    \n}\n";
    let offset = text.find("\n    \n").unwrap() + 5;
    let items = run(text, DslLanguage::Kotlin, offset);
    assert!(has_label(&items, "java"), "got {:?}", labels(&items));
    assert!(has_label(&items, "org.jetbrains.kotlin.jvm"));
    assert!(items.iter().all(|c| c.kind == CandidateKind::PluginId));
}

#[test]
fn repositories_block_offers_repo_functions_groovy() {
    let text = "repositories {\n    \n}\n";
    let offset = text.find("\n    \n").unwrap() + 5;
    let items = run(text, DslLanguage::Groovy, offset);
    assert!(has_label(&items, "mavenCentral"), "got {:?}", labels(&items));
    assert!(has_label(&items, "google"));
    assert!(items.iter().all(|c| c.kind == CandidateKind::Repository));
}

#[test]
fn libs_dot_offers_catalog_accessors_kotlin() {
    let text = "dependencies {\n    implementation(libs.)\n}\n";
    let offset = text.find("libs.").unwrap() + "libs.".len();
    let items = run_with_catalog(text, DslLanguage::Kotlin, offset);
    assert!(has_label(&items, "libs.guava"), "got {:?}", labels(&items));
    assert!(has_label(&items, "libs.commons.lang3"));
    assert!(
        items.iter().all(|c| c.kind == CandidateKind::CatalogAccessor),
        "after `libs.` ONLY catalog accessors: {:?}",
        labels(&items)
    );
}

#[test]
fn libs_dot_offers_catalog_accessors_groovy() {
    let text = "dependencies {\n    implementation libs.\n}\n";
    let offset = text.find("libs.").unwrap() + "libs.".len();
    let items = run_with_catalog(text, DslLanguage::Groovy, offset);
    assert!(has_label(&items, "libs.guava"), "got {:?}", labels(&items));
    assert!(items.iter().all(|c| c.kind == CandidateKind::CatalogAccessor));
}

#[test]
fn suppressed_inside_line_comment_returns_empty() {
    let text = "dependencies {\n    // a comment here\n}\n";
    let offset = text.find("comment").unwrap();
    let items = run(text, DslLanguage::Groovy, offset);
    assert!(items.is_empty(), "comment must suppress, got {:?}", labels(&items));
}

#[test]
fn suppressed_inside_string_literal_returns_empty() {
    let text = "dependencies {\n    implementation(\"group:artifact\")\n}\n";
    let offset = text.find("group:artifact").unwrap() + 3;
    let items = run(text, DslLanguage::Kotlin, offset);
    assert!(items.is_empty(), "string literal must suppress, got {:?}", labels(&items));
}

#[test]
fn suppressed_inside_opaque_region_returns_empty() {
    let text = "if (x) {\n    \n}\n";
    let offset = text.find("\n    \n").unwrap() + 5;
    let items = run(text, DslLanguage::Kotlin, offset);
    assert!(items.is_empty(), "opaque region must suppress, got {:?}", labels(&items));
}

#[test]
fn dsl_parity_dependencies_block_both_dsls() {
    let kt = "dependencies {\n    \n}\n";
    let gr = "dependencies {\n    \n}\n";
    let kt_offset = kt.find("\n    \n").unwrap() + 5;
    let gr_offset = gr.find("\n    \n").unwrap() + 5;
    let kt_items = run(kt, DslLanguage::Kotlin, kt_offset);
    let gr_items = run(gr, DslLanguage::Groovy, gr_offset);
    for config in ["implementation", "api", "testImplementation"] {
        assert!(has_label(&kt_items, config), "kotlin missing {config}: {:?}", labels(&kt_items));
        assert!(has_label(&gr_items, config), "groovy missing {config}: {:?}", labels(&gr_items));
    }
}

#[test]
fn ranking_is_a_separate_deterministic_pass() {
    let eligible = vec![
        Candidate::new("repositories", CandidateKind::BlockKeyword, ""),
        Candidate::new("zzz", CandidateKind::DependencyConfiguration, ""),
        Candidate::new("api", CandidateKind::DependencyConfiguration, ""),
        Candidate::new("dependencies", CandidateKind::BlockKeyword, ""),
    ];
    let ranked = super::ranking::rank(eligible.clone(), 50);
    assert_eq!(
        labels(&ranked),
        ["dependencies", "repositories", "api", "zzz"],
        "ranking sorts by (kind group, label), stable + independent of input order"
    );

    let again = super::ranking::rank(eligible, 50);
    assert_eq!(labels(&ranked), labels(&again));
}

#[test]
fn ranking_caps_to_max_candidates() {
    let eligible: Vec<Candidate> = (0..10)
        .map(|i| Candidate::new(format!("kw{i:02}"), CandidateKind::BlockKeyword, ""))
        .collect();
    let ranked = super::ranking::rank(eligible, 3);
    assert_eq!(ranked.len(), 3, "cap applied");
    assert_eq!(labels(&ranked), ["kw00", "kw01", "kw02"], "cap keeps the lowest-sorted");
}

#[test]
fn malformed_unclosed_file_never_panics_and_degrades() {
    // An unclosed `dependencies {` whose recovered closure span does not reach the cursor
    // degrades to a sensible result (here: top-level keywords) — never a panic, never garbage.
    let text = "dependencies {\n    \n";
    let offset = text.len() - 1;
    let items = run(text, DslLanguage::Groovy, offset);
    assert!(
        items.iter().all(|c| !c.label.is_empty()),
        "every candidate is well-formed (no garbage), got {:?}",
        labels(&items)
    );
}

#[test]
fn offset_past_end_is_safe() {
    let text = "plugins {}\n";
    let items = run(text, DslLanguage::Kotlin, text.len() + 100);
    assert!(items.iter().all(|c| c.kind == CandidateKind::BlockKeyword) || items.is_empty());
}
