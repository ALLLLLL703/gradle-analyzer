//! Manual-QA demo for the Task 7 static semantic graph.
//!
//! Run with:
//!
//! ```text
//! cargo run --example semantic_demo
//! ```
//!
//! It analyzes a small MULTI-FILE workspace — a settings script, a Kotlin build script, a
//! Groovy build script, a version catalog, a buildSrc script, and a deliberately malformed
//! script — over the real public [`analyze_documents`] API, then prints every
//! `SemanticGraph` fact with its id, parent_id, source span, status, and (for dependencies)
//! resolved-vs-unresolved catalog status.
//!
//! The binary-observable PASS is: a `libs.foo` dependency prints RESOLVED to its catalog
//! coordinate; plugins/tasks/includes print with a stable id and a correct source span; and
//! the malformed fixture prints partial facts with NO panic.

use gradle_analyzer::gradle::semantic::{
    DependencyCoordinate, FactPayload, FactStatus, SemanticDocument,
    SemanticGraph, SemanticInput, analyze_documents, describe_resolution,
};
use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
use gradle_analyzer::i18n::Translator;

/// A version catalog with a guava library reachable as `libs.guava`.
const CATALOG: &str = "\
[versions]
guava = \"33.0.0-jre\"

[libraries]
guava = { module = \"com.google.guava:guava\", version.ref = \"guava\" }

[bundles]
networking = [\"guava\"]
";

/// A settings script declaring the project graph.
const SETTINGS: &str = "\
rootProject.name = \"demo\"
include(\":app\")
include(\":core\")
";

/// A Kotlin build script: plugins, repos, a resolved + an unresolved accessor, a task.
const KT_BUILD: &str = "\
import org.gradle.api.tasks.testing.Test

plugins {
    id(\"java\")
    kotlin(\"jvm\") version \"1.9.22\"
}

repositories {
    mavenCentral()
}

dependencies {
    implementation(libs.guava)
    implementation(libs.nope)
    api(project(\":core\"))
}

tasks.register<Test>(\"integrationTest\") { }
";

/// A Groovy build script: plugin, repo, string + map dependency, a task.
const GV_BUILD: &str = "\
plugins {
    id 'application'
}
repositories {
    google()
}
dependencies {
    implementation 'org.apache.commons:commons-lang3:3.14.0'
    implementation group: 'com.google.guava', name: 'guava', version: '33.0.0-jre'
}
task hello { }
";

/// A buildSrc script contributing a local task symbol.
const BUILD_SRC: &str = "\
tasks.register(\"buildSrcHello\") { }
";

/// A malformed script: an opaque control-flow region plus an empty-coordinate dependency.
const MALFORMED: &str = "\
plugins {
    id(\"java\")
}
if (x) { y }
dependencies {
    implementation()
";

fn main() {
    println!("=== gradle-analyzer static semantic graph demo ===\n");
    let graph = analyze();
    let translator = Translator::new();

    for document in graph.documents() {
        print_document(document, &translator);
    }

    println!("\n{}", verdict(&graph));
}

/// Analyzes the multi-file fixture set into a graph.
fn analyze() -> SemanticGraph {
    let inputs = vec![
        SemanticInput::script("gradle/libs.versions.toml", CATALOG, GradleFileKind::VersionCatalog),
        SemanticInput::script(
            "settings.gradle.kts",
            SETTINGS,
            GradleFileKind::SettingsScript(DslLanguage::Kotlin),
        ),
        SemanticInput::script(
            "build.gradle.kts",
            KT_BUILD,
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        ),
        SemanticInput::script(
            "app/build.gradle",
            GV_BUILD,
            GradleFileKind::SubprojectBuildScript(DslLanguage::Groovy),
        ),
        SemanticInput::script(
            "buildSrc/build.gradle.kts",
            BUILD_SRC,
            GradleFileKind::BuildSrcScript(DslLanguage::Kotlin),
        ),
        SemanticInput::script(
            "broken/build.gradle.kts",
            MALFORMED,
            GradleFileKind::SubprojectBuildScript(DslLanguage::Kotlin),
        ),
    ];
    analyze_documents(&inputs)
}

/// Prints one document's facts: id, parent, source span, status, and resolution.
fn print_document(document: &SemanticDocument, translator: &Translator) {
    println!("########## {} ##########", document.id().as_str());
    if document.had_catalog_parse_error() {
        println!("  (version catalog failed to parse)");
    }
    for fact in document.facts() {
        let parent = fact
            .metadata
            .parent_id
            .as_ref()
            .map(|id| id.as_str())
            .unwrap_or("-");
        let span = fact.metadata.source;
        println!(
            "  [{:?}] {}\n      parent={} source={}..{} status={:?}",
            fact.kind(),
            fact.id().as_str(),
            parent,
            span.start,
            span.end(),
            fact.status,
        );
        if let FactPayload::Dependency { coordinate, .. } = &fact.payload {
            print_coordinate(coordinate, translator);
        }
    }
    println!();
}

/// Prints a dependency's coordinate, rendering accessor resolution via the i18n boundary.
fn print_coordinate(coordinate: &DependencyCoordinate, translator: &Translator) {
    match coordinate {
        DependencyCoordinate::StringNotation(coord) => {
            println!("      coordinate(string)={coord}");
        }
        DependencyCoordinate::CatalogAccessor { accessor, resolution } => {
            let status = if resolution.is_resolved() { "RESOLVED" } else { "UNRESOLVED" };
            println!(
                "      coordinate(catalog)={accessor} [{status}] -> {}",
                describe_resolution(translator, accessor, resolution),
            );
        }
        DependencyCoordinate::ProjectRef(path) => {
            println!("      coordinate(project)={path}");
        }
        DependencyCoordinate::Unknown => {
            println!("      coordinate(unknown) — partial");
        }
    }
}

/// The binary-observable verdict line.
fn verdict(graph: &SemanticGraph) -> String {
    let resolved_libs_guava = graph.all_facts().any(|f| match &f.payload {
        FactPayload::Dependency {
            coordinate: DependencyCoordinate::CatalogAccessor { accessor, resolution },
            ..
        } => accessor == "libs.guava" && resolution.is_resolved(),
        _ => false,
    });
    let unresolved_libs_nope = graph.all_facts().any(|f| match &f.payload {
        FactPayload::Dependency {
            coordinate: DependencyCoordinate::CatalogAccessor { accessor, resolution },
            ..
        } => accessor == "libs.nope" && !resolution.is_resolved(),
        _ => false,
    });
    let buildsrc_symbol = graph.all_facts().any(|f| matches!(
        &f.payload,
        FactPayload::BuildSrcSymbol { name, .. } if name == "buildSrcHello"
    ));
    let has_partial = graph.all_facts().any(|f| f.status == FactStatus::Partial);

    if resolved_libs_guava && unresolved_libs_nope && buildsrc_symbol && has_partial {
        "PASS: libs.guava RESOLVED, libs.nope UNRESOLVED, buildSrc symbol visible, \
         malformed input yielded partial facts (no panic)."
            .into()
    } else {
        format!(
            "CHECK: guava_resolved={resolved_libs_guava} nope_unresolved={unresolved_libs_nope} \
             buildsrc={buildsrc_symbol} partial={has_partial}"
        )
    }
}

/// The demo doubles as a smoke test so `cargo test` exercises the observable PASS path.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_proves_resolution_buildsrc_and_partial_paths() {
        let graph = analyze();

        let guava_resolved = graph.all_facts().any(|f| matches!(
            &f.payload,
            FactPayload::Dependency {
                coordinate: DependencyCoordinate::CatalogAccessor { accessor, resolution },
                ..
            } if accessor == "libs.guava" && resolution.is_resolved()
        ));
        assert!(guava_resolved, "libs.guava resolves to its catalog coordinate");

        let nope_unresolved = graph.all_facts().any(|f| matches!(
            &f.payload,
            FactPayload::Dependency {
                coordinate: DependencyCoordinate::CatalogAccessor { accessor, resolution },
                ..
            } if accessor == "libs.nope" && !resolution.is_resolved()
        ));
        assert!(nope_unresolved, "libs.nope is recorded unresolved");

        let buildsrc = graph.all_facts().any(|f| matches!(
            &f.payload,
            FactPayload::BuildSrcSymbol { name, .. } if name == "buildSrcHello"
        ));
        assert!(buildsrc, "buildSrc task symbol is visible");

        assert!(
            graph.all_facts().any(|f| f.status == FactStatus::Partial),
            "malformed fixture produced a partial fact without panicking"
        );
    }
}
