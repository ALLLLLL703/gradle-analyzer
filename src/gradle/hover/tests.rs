//! Tests for static hover: localized content per fact/block, none on opaque positions.

use crate::gradle::hover::hover;
use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{SemanticGraph, SemanticInput, analyze_documents};
use crate::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
use crate::i18n::MessageKey;

/// Builds a build-script document keyed by file name plus its analyzed graph.
fn setup(
    name: &str,
    text: &str,
    language: DslLanguage,
) -> (TrackedDocument, crate::gradle::syntax::Parse, SemanticGraph) {
    let kind = GradleFileKind::RootBuildScript(language);
    let uri = tower_lsp::lsp_types::Url::from_file_path(format!("/proj/{name}")).unwrap();
    let doc = TrackedDocument::new(uri, 1, text, kind);
    let parse = match language {
        DslLanguage::Kotlin => parse_kotlin(text),
        DslLanguage::Groovy => parse_groovy(text),
    };
    let input = SemanticInput::script(name, text, kind);
    let graph = analyze_documents(std::slice::from_ref(&input));
    (doc, parse, graph)
}

/// Builds a graph including a version catalog so `libs.*` accessors resolve.
fn setup_with_catalog(
    name: &str,
    text: &str,
    catalog: &str,
) -> (TrackedDocument, crate::gradle::syntax::Parse, SemanticGraph) {
    let kind = GradleFileKind::RootBuildScript(DslLanguage::Kotlin);
    let uri = tower_lsp::lsp_types::Url::from_file_path(format!("/proj/{name}")).unwrap();
    let doc = TrackedDocument::new(uri, 1, text, kind);
    let parse = parse_kotlin(text);
    let catalog_input =
        SemanticInput::script("libs.versions.toml", catalog, GradleFileKind::VersionCatalog);
    let script_input = SemanticInput::script(name, text, kind);
    let graph = analyze_documents(&[catalog_input, script_input]);
    (doc, parse, graph)
}

#[test]
fn hover_on_dependency_shows_localized_notation() {
    let text = "dependencies {\n    implementation(\"com.google.guava:guava:33.0\")\n}\n";
    let (doc, parse, graph) = setup("build.gradle.kts", text, DslLanguage::Kotlin);

    let offset = text.find("implementation").unwrap() + 2;
    let model = hover(&doc, &parse, &graph, offset).expect("dependency hover");
    assert_eq!(model.message_key, MessageKey::HoverDependency);
    assert_eq!(model.args[0], "implementation");
    assert_eq!(model.args[1], "com.google.guava:guava:33.0");
}

#[test]
fn hover_on_task_shows_task_summary() {
    let text = "task build {}\n";
    let (doc, parse, graph) = setup("build.gradle", text, DslLanguage::Groovy);

    let offset = text.find("build").unwrap() + 1;
    let model = hover(&doc, &parse, &graph, offset).expect("task hover");
    assert_eq!(model.message_key, MessageKey::HoverTask);
    assert_eq!(model.args[0], "build");
}

#[test]
fn hover_on_block_keyword_explains_the_block() {
    let text = "dependencies {\n    implementation(\"a:b:1.0\")\n}\n";
    let (doc, parse, graph) = setup("build.gradle.kts", text, DslLanguage::Kotlin);

    // Offset on the `dependencies` keyword itself (not its contents).
    let offset = 2;
    let model = hover(&doc, &parse, &graph, offset).expect("block keyword hover");
    assert_eq!(model.message_key, MessageKey::HoverBlockDependencies);
}

#[test]
fn hover_on_resolved_catalog_accessor_shows_coordinate() {
    let catalog = "[libraries]\nguava = \"com.google.guava:guava:33.0\"\n";
    let text = "dependencies {\n    implementation(libs.guava)\n}\n";
    let (doc, parse, graph) = setup_with_catalog("build.gradle.kts", text, catalog);

    let offset = text.find("libs.guava").unwrap() + 2;
    let model = hover(&doc, &parse, &graph, offset).expect("catalog accessor hover");
    assert_eq!(model.message_key, MessageKey::SemanticCatalogResolved);
    assert_eq!(model.args[0], "libs.guava");
    assert!(model.args[1].contains("guava"));
}

#[test]
fn hover_returns_none_on_opaque_position() {
    let text = "task build {}\n";
    let (doc, parse, graph) = setup("build.gradle", text, DslLanguage::Groovy);

    // Offset on trailing whitespace / newline — no fact, no block keyword.
    let offset = text.len() - 1;
    assert!(hover(&doc, &parse, &graph, offset).is_none());
}

#[test]
fn hover_returns_none_for_non_dsl_document() {
    let text = "[versions]\nguava = \"33.0\"\n";
    let uri = tower_lsp::lsp_types::Url::from_file_path("/proj/gradle/libs.versions.toml").unwrap();
    let doc = TrackedDocument::new(uri, 1, text, GradleFileKind::VersionCatalog);
    let parse = parse_groovy(text);
    let graph = SemanticGraph::new();
    assert!(hover(&doc, &parse, &graph, 1).is_none());
}

#[test]
fn hover_on_malformed_input_never_panics() {
    let text = "dependencies {{{ @@@ task ( plugins\n";
    let (doc, parse, graph) = setup("build.gradle", text, DslLanguage::Groovy);
    for offset in 0..text.len() {
        let _ = hover(&doc, &parse, &graph, offset);
    }
}
