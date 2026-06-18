//! Behavior specification for Task 12 navigation (both DSLs), written failing-first.
//!
//! Each test analyzes a small inline fixture into a [`SemanticGraph`], parses the same text,
//! then asserts goto-definition / find-references behavior at precise byte offsets. The
//! confidence guarantee (EMPTY over a guess) and malformed-not-panic degradation are asserted
//! explicitly so the verifier can see the negative cases ran.

use super::*;
use crate::gradle::syntax::Parse;
use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{DocumentId, SemanticInput, analyze_documents};
use crate::gradle::workspace::{DslLanguage, GradleFileKind};

/// Parses + analyzes one script as the sole document, returning its parse, graph, and nav doc.
fn prepare(id: &str, text: &str, lang: DslLanguage) -> (Parse, SemanticGraph, NavDocument) {
    let kind = match lang {
        DslLanguage::Kotlin => GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        DslLanguage::Groovy => GradleFileKind::RootBuildScript(DslLanguage::Groovy),
    };
    let graph = analyze_documents(&[SemanticInput::script(id, text, kind)]);
    let parse = match lang {
        DslLanguage::Kotlin => parse_kotlin(text),
        DslLanguage::Groovy => parse_groovy(text),
    };
    (parse, graph, NavDocument::new(DocumentId::new(id), lang))
}

/// Asserts every returned target's span covers a slice containing `expect` in `text`.
fn assert_target_text(targets: &[NavTarget], text: &str, expect: &str) {
    assert!(!targets.is_empty(), "expected at least one target");
    assert!(
        targets.iter().any(|t| {
            let NavTarget::Local { span, .. } = t;
            span.text(text).contains(expect)
        }),
        "no target span contains {expect:?}; targets={targets:?}"
    );
}

// --- goto-definition: task reference -> declaration (both DSLs) ---

#[test]
fn kotlin_dependson_reference_goes_to_task_declaration() {
    let text = "tasks.register(\"build\") {}\n\
                tasks.register(\"check\") {\n  dependsOn(\"build\")\n}\n";
    let (parse, graph, doc) = prepare("build.gradle.kts", text, DslLanguage::Kotlin);

    let offset = text.find("dependsOn(\"build\")").unwrap() + "dependsOn(\"".len() + 1;
    let targets = goto_definition(&doc, &parse, &graph, offset);
    assert_target_text(&targets, text, "build");
    // The target is the DECLARATION call, not the dependsOn reference site.
    let NavTarget::Local { span, .. } = &targets[0];
    assert!(span.text(text).contains("register"), "lands on the register decl");
}

#[test]
fn groovy_dependson_reference_goes_to_task_declaration() {
    let text = "task build {}\ntask check {\n  dependsOn 'build'\n}\n";
    let (parse, graph, doc) = prepare("build.gradle", text, DslLanguage::Groovy);

    let offset = text.find("dependsOn 'build'").unwrap() + "dependsOn '".len() + 1;
    let targets = goto_definition(&doc, &parse, &graph, offset);
    assert_target_text(&targets, text, "build");
    // Declaration site is `task build {}` (earlier than the reference).
    let NavTarget::Local { span, .. } = &targets[0];
    assert!(span.start < text.find("dependsOn").unwrap(), "decl precedes the ref");
}

// --- goto-definition: libs.* accessor -> catalog document ---

#[test]
fn kotlin_libs_accessor_goes_to_catalog_document() {
    let catalog = "[libraries]\nguava = \"com.google.guava:guava:33.0.0-jre\"\n";
    let build = "dependencies {\n  implementation(libs.guava)\n}\n";
    let graph = analyze_documents(&[
        SemanticInput::script("gradle/libs.versions.toml", catalog, GradleFileKind::VersionCatalog),
        SemanticInput::script(
            "build.gradle.kts",
            build,
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        ),
    ]);
    let parse = parse_kotlin(build);
    let doc = NavDocument::new(DocumentId::new("build.gradle.kts"), DslLanguage::Kotlin);

    let offset = build.find("libs.guava").unwrap() + "libs.".len() + 1;
    let targets = goto_definition(&doc, &parse, &graph, offset);
    assert!(!targets.is_empty(), "accessor resolves to a catalog target");
    let NavTarget::Local { document, .. } = &targets[0];
    assert_eq!(document.as_str(), "gradle/libs.versions.toml");
}

#[test]
fn groovy_libs_accessor_goes_to_catalog_document() {
    let catalog = "[libraries]\nguava = \"com.google.guava:guava:33.0.0-jre\"\n";
    let build = "dependencies {\n  implementation libs.guava\n}\n";
    let graph = analyze_documents(&[
        SemanticInput::script("gradle/libs.versions.toml", catalog, GradleFileKind::VersionCatalog),
        SemanticInput::script(
            "build.gradle",
            build,
            GradleFileKind::RootBuildScript(DslLanguage::Groovy),
        ),
    ]);
    let parse = parse_groovy(build);
    let doc = NavDocument::new(DocumentId::new("build.gradle"), DslLanguage::Groovy);

    let offset = build.find("libs.guava").unwrap() + "libs.".len() + 1;
    let targets = goto_definition(&doc, &parse, &graph, offset);
    assert!(!targets.is_empty(), "accessor resolves to a catalog target");
    let NavTarget::Local { document, .. } = &targets[0];
    assert_eq!(document.as_str(), "gradle/libs.versions.toml");
}

// --- goto-definition: project(":path") -> settings include ---

#[test]
fn kotlin_project_reference_goes_to_settings_include() {
    let settings = "include(\":core\")\ninclude(\":app\")\n";
    let build = "dependencies {\n  api(project(\":core\"))\n}\n";
    let graph = analyze_documents(&[
        SemanticInput::script(
            "settings.gradle.kts",
            settings,
            GradleFileKind::SettingsScript(DslLanguage::Kotlin),
        ),
        SemanticInput::script(
            "build.gradle.kts",
            build,
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        ),
    ]);
    let parse = parse_kotlin(build);
    let doc = NavDocument::new(DocumentId::new("build.gradle.kts"), DslLanguage::Kotlin);

    let offset = build.find("project(\":core\")").unwrap() + "project(\":".len();
    let targets = goto_definition(&doc, &parse, &graph, offset);
    assert!(!targets.is_empty(), "project ref resolves to a settings include");
    let NavTarget::Local { document, span } = &targets[0];
    assert_eq!(document.as_str(), "settings.gradle.kts");
    assert!(span.text(settings).contains("core"));
}

#[test]
fn groovy_project_reference_goes_to_settings_include() {
    let settings = "include ':core'\ninclude ':app'\n";
    let build = "dependencies {\n  api project(':core')\n}\n";
    let graph = analyze_documents(&[
        SemanticInput::script(
            "settings.gradle",
            settings,
            GradleFileKind::SettingsScript(DslLanguage::Groovy),
        ),
        SemanticInput::script(
            "build.gradle",
            build,
            GradleFileKind::RootBuildScript(DslLanguage::Groovy),
        ),
    ]);
    let parse = parse_groovy(build);
    let doc = NavDocument::new(DocumentId::new("build.gradle"), DslLanguage::Groovy);

    let offset = build.find("project(':core')").unwrap() + "project(':".len();
    let targets = goto_definition(&doc, &parse, &graph, offset);
    assert!(!targets.is_empty(), "project ref resolves to a settings include");
    let NavTarget::Local { document, span } = &targets[0];
    assert_eq!(document.as_str(), "settings.gradle");
    assert!(span.text(settings).contains("core"));
}

// --- find-references: from a declaration, list all reference sites (both DSLs) ---

#[test]
fn kotlin_find_references_from_declaration_lists_all_sites() {
    let text = "tasks.register(\"build\") {}\n\
                tasks.register(\"a\") { dependsOn(\"build\") }\n\
                tasks.register(\"b\") { dependsOn(\"build\") }\n";
    let (parse, graph, doc) = prepare("build.gradle.kts", text, DslLanguage::Kotlin);

    // Cursor on the declaration name string.
    let offset = text.find("register(\"build\")").unwrap() + "register(\"".len() + 1;
    let refs = find_references(&doc, &parse, &graph, offset);
    // Declaration + two dependsOn references = 3 sites.
    assert_eq!(refs.len(), 3, "decl + 2 refs; got {refs:?}");
    let dependson_sites = refs
        .iter()
        .filter(|t| {
            let NavTarget::Local { span, .. } = t;
            span.start > text.find("dependsOn").unwrap_or(usize::MAX).saturating_sub(1)
        })
        .count();
    assert!(dependson_sites >= 2, "both dependsOn sites present");
}

#[test]
fn groovy_find_references_from_declaration_lists_all_sites() {
    let text = "task build {}\n\
                task a { dependsOn 'build' }\n\
                task b { dependsOn 'build' }\n";
    let (parse, graph, doc) = prepare("build.gradle", text, DslLanguage::Groovy);

    let offset = text.find("task build").unwrap() + "task ".len() + 1;
    let refs = find_references(&doc, &parse, &graph, offset);
    assert_eq!(refs.len(), 3, "decl + 2 refs; got {refs:?}");
}

// --- confidence: unsupported / ambiguous / opaque positions return EMPTY ---

#[test]
fn plugin_id_string_is_not_a_navigable_position() {
    let text = "plugins {\n  id(\"java\")\n}\n";
    let (parse, graph, doc) = prepare("build.gradle.kts", text, DslLanguage::Kotlin);

    // Cursor inside the plugin id string is NOT a task/project/catalog occurrence.
    let offset = text.find("\"java\"").unwrap() + 2;
    assert!(
        goto_definition(&doc, &parse, &graph, offset).is_empty(),
        "plugin id is not navigable (no guess)"
    );
    assert!(find_references(&doc, &parse, &graph, offset).is_empty());
}

#[test]
fn position_in_whitespace_returns_empty() {
    let text = "task build {}\n\n\n";
    let (parse, graph, doc) = prepare("build.gradle", text, DslLanguage::Groovy);
    let offset = text.len() - 1; // trailing blank line
    assert!(goto_definition(&doc, &parse, &graph, offset).is_empty());
    assert!(find_references(&doc, &parse, &graph, offset).is_empty());
}

#[test]
fn unresolved_task_reference_yields_no_definition() {
    // A dependsOn naming a task that is never declared: a reference with no definition.
    let text = "tasks.register(\"build\") { dependsOn(\"ghost\") }\n";
    let (parse, graph, doc) = prepare("build.gradle.kts", text, DslLanguage::Kotlin);
    let offset = text.find("dependsOn(\"ghost\")").unwrap() + "dependsOn(\"".len() + 1;
    // The position IS a task reference, but no declaration exists -> empty (not a guess).
    assert!(
        goto_definition(&doc, &parse, &graph, offset).is_empty(),
        "no declaration for ghost -> empty"
    );
}

#[test]
fn unresolved_libs_accessor_yields_no_definition() {
    let catalog = "[libraries]\nguava = \"com.google.guava:guava:33.0.0-jre\"\n";
    let build = "dependencies {\n  implementation(libs.absent)\n}\n";
    let graph = analyze_documents(&[
        SemanticInput::script("gradle/libs.versions.toml", catalog, GradleFileKind::VersionCatalog),
        SemanticInput::script(
            "build.gradle.kts",
            build,
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        ),
    ]);
    let parse = parse_kotlin(build);
    let doc = NavDocument::new(DocumentId::new("build.gradle.kts"), DslLanguage::Kotlin);
    let offset = build.find("libs.absent").unwrap() + "libs.".len() + 1;
    assert!(
        goto_definition(&doc, &parse, &graph, offset).is_empty(),
        "undefined accessor -> empty"
    );
}

// --- adversarial: malformed input degrades to empty, never panics ---

#[test]
fn malformed_input_returns_empty_not_panic() {
    let broken = [
        ("a.gradle.kts", "tasks.register(\"build\"", DslLanguage::Kotlin),
        ("b.gradle", "task build {\n  dependsOn 'x'", DslLanguage::Groovy),
        ("c.gradle.kts", "dependencies { implementation(libs.", DslLanguage::Kotlin),
        ("d.gradle", "}}} include ':", DslLanguage::Groovy),
        ("e.gradle.kts", "", DslLanguage::Kotlin),
    ];
    for (id, text, lang) in broken {
        let (parse, graph, doc) = prepare(id, text, lang);
        for offset in [0usize, text.len() / 2, text.len().saturating_sub(1)] {
            // Must not panic; empty is acceptable.
            let _ = goto_definition(&doc, &parse, &graph, offset);
            let _ = find_references(&doc, &parse, &graph, offset);
        }
    }
}

// --- locate primitive: smallest-span wins, half-open containment ---

#[test]
fn locate_at_picks_smallest_containing_span() {
    use super::locate::{Occurrence, OccurrenceRole, Symbol};
    use crate::gradle::syntax::TextSpan;

    let occurrences = vec![
        Occurrence {
            span: TextSpan::new(0, 20),
            symbol: Symbol::Task("outer".into()),
            role: OccurrenceRole::Reference,
        },
        Occurrence {
            span: TextSpan::new(5, 5),
            symbol: Symbol::Task("inner".into()),
            role: OccurrenceRole::Reference,
        },
    ];
    let hit = super::locate::locate_at(&occurrences, 7).unwrap();
    assert_eq!(hit.symbol, Symbol::Task("inner".into()));
    assert!(super::locate::locate_at(&occurrences, 25).is_none());
}
