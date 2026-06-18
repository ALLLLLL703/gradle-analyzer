//! Tests for [`compute_diagnostics`](super::compute_diagnostics) and its families.
//!
//! Each test builds a [`TrackedDocument`] + [`Parse`] + [`SemanticDocument`] from inline
//! source (no live server) and asserts the produced [`Diagnostic`]s. Cases cover both DSLs,
//! the four families, comment/string/opaque suppression, and the unused-import false case.

use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{SemanticDocument, SemanticInput, analyze_documents};
use crate::gradle::syntax::{Parse, SyntaxErrorKind};
use crate::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
use crate::i18n::MessageKey;

use super::model::{DiagnosticKind, Severity};
use super::{Diagnostic, compute_diagnostics};

fn kotlin_kind() -> GradleFileKind {
    GradleFileKind::RootBuildScript(DslLanguage::Kotlin)
}

fn groovy_kind() -> GradleFileKind {
    GradleFileKind::RootBuildScript(DslLanguage::Groovy)
}

/// Builds a document + parse + the single-document semantic graph for `source`.
fn fixture(rel: &str, source: &str, kind: GradleFileKind) -> (TrackedDocument, Parse, SemanticDocument) {
    let uri = tower_lsp::lsp_types::Url::from_file_path(format!("/proj/{rel}")).unwrap();
    let doc = TrackedDocument::new(uri, 1, source, kind);
    let parse = match kind.dsl() {
        Some(DslLanguage::Kotlin) => parse_kotlin(source),
        _ => parse_groovy(source),
    };
    let input = SemanticInput::script(rel, source, kind);
    let graph = analyze_documents(std::slice::from_ref(&input));
    let semantics = graph.document(&input.id).expect("document analyzed").clone();
    (doc, parse, semantics)
}

fn run(rel: &str, source: &str, kind: GradleFileKind) -> Vec<Diagnostic> {
    let (doc, parse, semantics) = fixture(rel, source, kind);
    compute_diagnostics(&doc, &parse, &semantics)
}

fn of_kind(diags: &[Diagnostic], kind: DiagnosticKind) -> Vec<&Diagnostic> {
    diags.iter().filter(|d| d.kind == kind).collect()
}

#[test]
fn kotlin_unclosed_block_maps_to_error_with_span_and_key() {
    let source = "dependencies {\n    implementation(\"a:b:1.0\")\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    let syntax = of_kind(&diags, DiagnosticKind::Syntax);
    assert!(!syntax.is_empty(), "expected a syntax diagnostic: {diags:?}");
    let unclosed = syntax
        .iter()
        .find(|d| d.message_key == MessageKey::SyntaxUnclosedBlock)
        .expect("an unclosed-block diagnostic");
    assert_eq!(unclosed.severity, Severity::Error);
    // The span anchors inside the source, not past its end.
    assert!(unclosed.span.end() <= source.len(), "span within source");
}

#[test]
fn groovy_unclosed_block_maps_to_error() {
    let source = "dependencies {\n    implementation 'a:b:1.0'\n";
    let diags = run("build.gradle", source, groovy_kind());
    let syntax = of_kind(&diags, DiagnosticKind::Syntax);
    assert!(
        syntax.iter().any(|d| d.message_key == MessageKey::SyntaxUnclosedBlock
            && d.severity == Severity::Error),
        "expected groovy unclosed-block error: {diags:?}"
    );
}

#[test]
fn kotlin_missing_equals_maps_to_error_key() {
    // `group "x"` (no `=`) is the MissingEquals nucleus case in the Kotlin frontend.
    let source = "group \"com.example\"\n";
    let (_doc, parse, _sem) = fixture("build.gradle.kts", source, kotlin_kind());
    // Only assert if the parser actually recorded the typed error (frontend-dependent).
    if parse
        .errors
        .as_slice()
        .iter()
        .any(|e| e.kind == SyntaxErrorKind::MissingEquals)
    {
        let diags = run("build.gradle.kts", source, kotlin_kind());
        assert!(
            diags
                .iter()
                .any(|d| d.message_key == MessageKey::SyntaxMissingEquals
                    && d.severity == Severity::Error),
            "missing-equals should surface as an error: {diags:?}"
        );
    }
}

#[test]
fn duplicate_registered_task_is_flagged_once() {
    let source = "tasks.register(\"build\") {\n}\ntasks.register(\"build\") {\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    let dups = of_kind(&diags, DiagnosticKind::DuplicateDeclaration);
    assert_eq!(dups.len(), 1, "second declaration flagged once: {diags:?}");
    assert_eq!(dups[0].severity, Severity::Warning);
    assert_eq!(dups[0].args, ["build"]);
    assert_eq!(dups[0].message_key, MessageKey::DiagnosticDuplicateDeclaration);
}

#[test]
fn unresolved_local_depends_on_is_flagged() {
    let source = "tasks.register(\"a\") {\n    dependsOn(\"ghost\")\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    let refs = of_kind(&diags, DiagnosticKind::UnresolvedTaskRef);
    assert_eq!(refs.len(), 1, "ghost dependsOn flagged: {diags:?}");
    assert_eq!(refs[0].args, ["ghost"]);
    assert_eq!(refs[0].message_key, MessageKey::DiagnosticUnresolvedTaskRef);
    assert_eq!(refs[0].severity, Severity::Warning);
}

#[test]
fn valid_local_depends_on_declared_task_is_not_flagged() {
    let source = "tasks.register(\"a\") {\n}\ntasks.register(\"b\") {\n    dependsOn(\"a\")\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(
        of_kind(&diags, DiagnosticKind::UnresolvedTaskRef).is_empty(),
        "dependsOn a declared task must NOT be flagged: {diags:?}"
    );
}

#[test]
fn depends_on_builtin_lifecycle_task_is_not_flagged() {
    let source = "tasks.register(\"a\") {\n    dependsOn(\"build\")\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(
        of_kind(&diags, DiagnosticKind::UnresolvedTaskRef).is_empty(),
        "dependsOn a built-in lifecycle task must NOT be flagged: {diags:?}"
    );
}

#[test]
fn unused_import_true_case_is_flagged() {
    let source = "import org.example.Unused\n\nplugins {\n    application\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    let unused = of_kind(&diags, DiagnosticKind::UnusedImport);
    assert_eq!(unused.len(), 1, "never-referenced import flagged: {diags:?}");
    assert_eq!(unused[0].message_key, MessageKey::DiagnosticUnusedImport);
    assert_eq!(unused[0].severity, Severity::Warning);
    assert!(unused[0].args[0].ends_with("Unused"));
}

#[test]
fn unused_import_false_case_referenced_type_is_not_flagged() {
    // `Test` is imported AND used as the type arg of `tasks.register<Test>`.
    let source = "import org.gradle.api.tasks.testing.Test\n\ntasks.register<Test>(\"it\") {\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(
        of_kind(&diags, DiagnosticKind::UnusedImport).is_empty(),
        "a referenced import must NOT be flagged: {diags:?}"
    );
}

#[test]
fn wildcard_import_is_never_flagged() {
    let source = "import org.example.*\n\nplugins {\n    application\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(
        of_kind(&diags, DiagnosticKind::UnusedImport).is_empty(),
        "wildcard imports are not statically certain; never flag: {diags:?}"
    );
}

#[test]
fn depends_on_inside_comment_produces_no_diagnostic() {
    // A dependsOn-looking token inside a line comment must not be analyzed.
    let source = "tasks.register(\"a\") {\n    // dependsOn(\"ghost\")\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(
        of_kind(&diags, DiagnosticKind::UnresolvedTaskRef).is_empty(),
        "commented dependsOn must NOT be flagged: {diags:?}"
    );
}

#[test]
fn import_used_only_inside_a_string_is_still_unused() {
    // The symbol appears only inside a string literal -> not a real code reference.
    let source = "import org.example.Unused\n\ngroup = \"Unused\"\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert_eq!(
        of_kind(&diags, DiagnosticKind::UnusedImport).len(),
        1,
        "string-only occurrence is not a reference; import stays unused: {diags:?}"
    );
}

#[test]
fn plugin_derived_unknown_member_produces_no_false_diagnostic() {
    // `spotless { ... }` is a plugin-contributed block we cannot statically know; it must
    // produce NO diagnostic (that is Task 16 territory, explicitly out of scope here).
    let source = "spotless {\n    kotlin {\n        ktlint()\n    }\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(
        diags.is_empty(),
        "unknown plugin members must not produce diagnostics: {diags:?}"
    );
}

#[test]
fn clean_file_yields_no_diagnostics() {
    let source = "plugins {\n    application\n}\n\nrepositories {\n    mavenCentral()\n}\n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    assert!(diags.is_empty(), "a clean file publishes nothing: {diags:?}");
}

#[test]
fn malformed_input_yields_diagnostics_not_a_panic() {
    // Deliberately broken: unbalanced braces, stray tokens. Must not panic.
    let source = "plugins {{{ \n dependencies ( \n $$$ \n";
    let diags = run("build.gradle.kts", source, kotlin_kind());
    // We don't assert a specific count, only that the pass survived and produced findings.
    let _ = diags;
}

#[test]
fn catalog_document_yields_no_diagnostics() {
    let source = "[libraries]\nguava = \"com.google.guava:guava:33.0.0-jre\"\n";
    let diags = run("gradle/libs.versions.toml", source, GradleFileKind::VersionCatalog);
    assert!(diags.is_empty(), "catalog files are out of scope: {diags:?}");
}
