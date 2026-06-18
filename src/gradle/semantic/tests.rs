//! Behavior specification for the Task 7 static semantic graph (both DSLs).
//!
//! Golden-style: each test analyzes real fixtures (the Task 5/6 valid scripts plus small
//! focused additions) and asserts stable ids, parent/source ownership, catalog resolution,
//! buildSrc symbol visibility, deterministic duplicate suffixing, and partial-not-panic
//! degradation. Written failing-first against the empty module.

use super::*;
use crate::gradle::workspace::{DslLanguage, GradleFileKind};

const KT_BUILD: &str = include_str!("../../../tests/fixtures/kotlin/valid_build.gradle.kts");
const KT_SETTINGS: &str = include_str!("../../../tests/fixtures/kotlin/valid_settings.gradle.kts");
const GV_BUILD: &str = include_str!("../../../tests/fixtures/groovy/valid_build.gradle");
const GV_SETTINGS: &str = include_str!("../../../tests/fixtures/groovy/valid_settings.gradle");
const CATALOG: &str = include_str!("../../../tests/fixtures/catalog/libs.versions.toml");
const GARBAGE_CATALOG: &str = include_str!("../../../tests/fixtures/catalog/garbage.versions.toml");
const BUILD_SRC: &str = include_str!("../../../tests/fixtures/buildsrc/build.gradle.kts");
const KT_PARTIAL: &str = include_str!("../../../tests/fixtures/kotlin/semantic_partial.gradle.kts");

/// Builds a catalog input from the canonical fixture.
fn catalog_input() -> SemanticInput {
    SemanticInput::script("gradle/libs.versions.toml", CATALOG, GradleFileKind::VersionCatalog)
}

/// Counts facts of a kind in a document.
fn count(doc: &SemanticDocument, kind: SemanticFactKind) -> usize {
    doc.facts_of_kind(kind).count()
}

#[test]
fn extracts_kotlin_build_nucleus_with_stable_ids_and_source() {
    let input = SemanticInput::script(
        "build.gradle.kts",
        KT_BUILD,
        GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
    );
    let graph = analyze_documents(&[input]);
    let doc = graph.document(&DocumentId::new("build.gradle.kts")).expect("doc present");

    assert!(count(doc, SemanticFactKind::Plugin) >= 3, "plugins extracted");
    assert!(count(doc, SemanticFactKind::Repository) >= 2, "repositories extracted");
    assert!(count(doc, SemanticFactKind::Dependency) >= 4, "dependencies extracted");
    assert!(count(doc, SemanticFactKind::Task) >= 2, "tasks extracted");
    assert!(count(doc, SemanticFactKind::Import) >= 1, "import extracted");

    // A stable, greppable id with correct source ownership.
    let java_like = doc
        .facts_of_kind(SemanticFactKind::Plugin)
        .find(|f| f.id().as_str().contains("plugin:"))
        .expect("a plugin fact");
    assert!(java_like.id().as_str().starts_with("build.gradle.kts::plugin:"));
    // The source span points back into the script (non-empty, within bounds).
    assert!(java_like.metadata.source.len > 0);
    assert!(java_like.metadata.source.end() <= KT_BUILD.len());
}

#[test]
fn extracts_groovy_build_nucleus_with_stable_ids_and_source() {
    let input = SemanticInput::script(
        "build.gradle",
        GV_BUILD,
        GradleFileKind::RootBuildScript(DslLanguage::Groovy),
    );
    let graph = analyze_documents(&[input]);
    let doc = graph.document(&DocumentId::new("build.gradle")).unwrap();

    assert!(count(doc, SemanticFactKind::Plugin) >= 2, "id 'java'/'application'");
    assert!(count(doc, SemanticFactKind::Repository) >= 2, "mavenCentral/google");
    assert!(count(doc, SemanticFactKind::Dependency) >= 3, "string + map notation");
    assert!(count(doc, SemanticFactKind::Task) >= 1, "task hello");

    // The Groovy map-notation dependency resolves to a g:a:v coordinate.
    let map_dep = doc
        .facts_of_kind(SemanticFactKind::Dependency)
        .find_map(|f| match &f.payload {
            FactPayload::Dependency { coordinate: DependencyCoordinate::StringNotation(c), .. }
                if c.contains("guava") =>
            {
                Some(c.clone())
            }
            _ => None,
        });
    assert_eq!(map_dep.as_deref(), Some("com.google.guava:guava:33.0.0-jre"));
}

#[test]
fn extracts_includes_and_root_project_name_from_both_settings() {
    let kt = SemanticInput::script(
        "settings.gradle.kts",
        KT_SETTINGS,
        GradleFileKind::SettingsScript(DslLanguage::Kotlin),
    );
    let gv = SemanticInput::script(
        "settings.gradle",
        GV_SETTINGS,
        GradleFileKind::SettingsScript(DslLanguage::Groovy),
    );
    let graph = analyze_documents(&[kt, gv]);

    let kt_doc = graph.document(&DocumentId::new("settings.gradle.kts")).unwrap();
    assert!(count(kt_doc, SemanticFactKind::ProjectInclude) >= 3, "include x3");
    assert_eq!(count(kt_doc, SemanticFactKind::RootProjectName), 1, "rootProject.name");

    let gv_doc = graph.document(&DocumentId::new("settings.gradle")).unwrap();
    // include ':app' + include ':core', ':feature' => 3 project includes.
    assert!(count(gv_doc, SemanticFactKind::ProjectInclude) >= 3, "groovy includes");
    assert_eq!(count(gv_doc, SemanticFactKind::RootProjectName), 1);

    // rootProject.name carries the literal name.
    let name = gv_doc
        .facts_of_kind(SemanticFactKind::RootProjectName)
        .find_map(|f| match &f.payload {
            FactPayload::RootProjectName(n) => Some(n.clone()),
            _ => None,
        });
    assert_eq!(name.as_deref(), Some("my-app"));
}

#[test]
fn libs_accessor_resolves_to_catalog_entry_kotlin() {
    let build = SemanticInput::script(
        "build.gradle.kts",
        "dependencies {\n    implementation(libs.guava)\n    implementation(libs.bundles.networking)\n}",
        GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
    );
    let graph = analyze_documents(&[catalog_input(), build]);
    let doc = graph.document(&DocumentId::new("build.gradle.kts")).unwrap();

    let resolved: Vec<_> = doc
        .facts_of_kind(SemanticFactKind::Dependency)
        .filter_map(|f| match &f.payload {
            FactPayload::Dependency {
                coordinate: DependencyCoordinate::CatalogAccessor { accessor, resolution },
                ..
            } => Some((accessor.clone(), resolution.clone())),
            _ => None,
        })
        .collect();

    let guava = resolved.iter().find(|(a, _)| a == "libs.guava").expect("libs.guava accessor");
    match &guava.1 {
        CatalogResolution::Resolved { coordinate, .. } => {
            assert_eq!(coordinate, "com.google.guava:guava:33.0.0-jre", "RESOLVED to coordinate");
        }
        CatalogResolution::Unresolved => panic!("libs.guava must resolve"),
    }

    let bundle = resolved
        .iter()
        .find(|(a, _)| a == "libs.bundles.networking")
        .expect("bundle accessor");
    assert!(bundle.1.is_resolved(), "bundle resolves");
}

#[test]
fn libs_accessor_resolves_to_catalog_entry_groovy() {
    let build = SemanticInput::script(
        "build.gradle",
        "dependencies {\n    implementation libs.commons.lang3\n}",
        GradleFileKind::RootBuildScript(DslLanguage::Groovy),
    );
    let graph = analyze_documents(&[catalog_input(), build]);
    let doc = graph.document(&DocumentId::new("build.gradle")).unwrap();

    let resolution = doc
        .facts_of_kind(SemanticFactKind::Dependency)
        .find_map(|f| match &f.payload {
            FactPayload::Dependency {
                coordinate: DependencyCoordinate::CatalogAccessor { resolution, .. },
                ..
            } => Some(resolution.clone()),
            _ => None,
        })
        .expect("a catalog accessor dependency");
    match resolution {
        CatalogResolution::Resolved { coordinate, .. } => {
            assert_eq!(coordinate, "org.apache.commons:commons-lang3:3.14.0");
        }
        CatalogResolution::Unresolved => panic!("libs.commons.lang3 must resolve"),
    }
}

#[test]
fn undefined_libs_accessor_is_recorded_unresolved_not_a_panic() {
    let build = SemanticInput::script(
        "build.gradle.kts",
        "dependencies {\n    implementation(libs.nope)\n}",
        GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
    );
    let graph = analyze_documents(&[catalog_input(), build]);
    let doc = graph.document(&DocumentId::new("build.gradle.kts")).unwrap();

    let resolution = doc
        .facts_of_kind(SemanticFactKind::Dependency)
        .find_map(|f| match &f.payload {
            FactPayload::Dependency {
                coordinate: DependencyCoordinate::CatalogAccessor { resolution, .. },
                ..
            } => Some(resolution.clone()),
            _ => None,
        })
        .expect("a catalog accessor dependency");
    assert_eq!(resolution, CatalogResolution::Unresolved, "libs.nope is UNRESOLVED");
}

#[test]
fn catalog_entries_are_facts_with_library_version_parent_links() {
    let graph = analyze_documents(&[catalog_input()]);
    let doc = graph.document(&DocumentId::new("gradle/libs.versions.toml")).unwrap();

    assert!(count(doc, SemanticFactKind::CatalogVersion) >= 3, "versions");
    assert!(count(doc, SemanticFactKind::CatalogLibrary) >= 3, "libraries");
    assert!(count(doc, SemanticFactKind::CatalogBundle) >= 1, "bundles");
    assert!(count(doc, SemanticFactKind::CatalogPlugin) >= 2, "plugins");

    // guava library parents to the guava version entry (version.ref ownership).
    let guava_lib = doc
        .facts_of_kind(SemanticFactKind::CatalogLibrary)
        .find(|f| matches!(&f.payload, FactPayload::CatalogLibrary { alias, .. } if alias == "guava"))
        .expect("guava library fact");
    let parent = guava_lib.metadata.parent_id.clone().expect("guava library has a version parent");
    let version_fact = doc.fact(&parent).expect("parent version fact resolvable");
    assert!(matches!(&version_fact.payload, FactPayload::CatalogVersion { alias, .. } if alias == "guava"));
}

#[test]
fn build_src_declared_task_symbol_is_visible() {
    let input = SemanticInput::script(
        "buildSrc/build.gradle.kts",
        BUILD_SRC,
        GradleFileKind::BuildSrcScript(DslLanguage::Kotlin),
    );
    let graph = analyze_documents(&[input]);
    let doc = graph.document(&DocumentId::new("buildSrc/build.gradle.kts")).unwrap();

    let symbols: Vec<_> = doc
        .facts_of_kind(SemanticFactKind::BuildSrcSymbol)
        .filter_map(|f| match &f.payload {
            FactPayload::BuildSrcSymbol { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(symbols.iter().any(|n| n == "buildSrcHello"), "task symbol visible, got {symbols:?}");
}

#[test]
fn duplicate_keys_under_one_parent_get_deterministic_suffixes() {
    let build = SemanticInput::script(
        "build.gradle",
        "repositories {\n    mavenCentral()\n    mavenCentral()\n    mavenCentral()\n}",
        GradleFileKind::RootBuildScript(DslLanguage::Groovy),
    );
    let graph = analyze_documents(&[build]);
    let doc = graph.document(&DocumentId::new("build.gradle")).unwrap();

    let ids: Vec<String> = doc
        .facts_of_kind(SemanticFactKind::Repository)
        .map(|f| f.id().as_str().to_string())
        .collect();
    assert_eq!(
        ids,
        vec![
            "build.gradle::repository:mavenCentral",
            "build.gradle::repository:mavenCentral#2",
            "build.gradle::repository:mavenCentral#3",
        ],
        "deterministic #2/#3 suffixing"
    );
}

#[test]
fn malformed_partial_input_yields_partial_facts_and_never_panics() {
    let input = SemanticInput::script(
        "build.gradle.kts",
        KT_PARTIAL,
        GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
    );
    // The whole point: this must not panic.
    let graph = analyze_documents(&[input]);
    let doc = graph.document(&DocumentId::new("build.gradle.kts")).unwrap();

    // The opaque `if` region is skipped; the surrounding nucleus still extracts.
    assert!(count(doc, SemanticFactKind::Plugin) >= 1, "plugin before opaque region");
    // `implementation()` with no coordinate degrades to a Partial dependency, not a panic.
    let has_partial = doc
        .facts_of_kind(SemanticFactKind::Dependency)
        .any(|f| f.status == FactStatus::Partial);
    assert!(has_partial, "an empty-coordinate dependency is Partial");
}

#[test]
fn garbage_catalog_degrades_to_zero_facts_with_parse_error_flag() {
    let input = SemanticInput::script(
        "gradle/libs.versions.toml",
        GARBAGE_CATALOG,
        GradleFileKind::VersionCatalog,
    );
    let graph = analyze_documents(&[input]);
    let doc = graph.document(&DocumentId::new("gradle/libs.versions.toml")).unwrap();
    assert!(doc.had_catalog_parse_error(), "garbage TOML flags a parse error");
    assert_eq!(doc.facts().len(), 0, "no catalog facts from garbage input");
}

#[test]
fn ids_are_stable_across_re_analysis_of_identical_input() {
    let build = || {
        SemanticInput::script(
            "build.gradle.kts",
            KT_BUILD,
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        )
    };
    let collect_ids = || {
        let graph = analyze_documents(&[catalog_input(), build()]);
        graph
            .all_facts()
            .map(|f| f.id().as_str().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(collect_ids(), collect_ids(), "identical input => identical ids");
}
