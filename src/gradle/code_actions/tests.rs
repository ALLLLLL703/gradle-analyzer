//! Tests for the code-action whitelist: each action only under its exact precondition.

use crate::gradle::code_actions::code_actions;
use crate::gradle::diagnostics::compute_diagnostics;
use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{SemanticGraph, SemanticInput, analyze_documents};
use crate::gradle::syntax::{Parse, TextSpan};
use crate::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
use crate::i18n::MessageKey;

use super::{CodeActionCategory, CodeActionModel};

/// Builds a Kotlin build-script document keyed by the given file name.
fn kotlin_doc(name: &str, text: &str) -> TrackedDocument {
    let uri = tower_lsp::lsp_types::Url::from_file_path(format!("/proj/{name}")).unwrap();
    TrackedDocument::new(
        uri,
        1,
        text,
        GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
    )
}

/// Builds a Groovy build-script document keyed by the given file name.
fn groovy_doc(name: &str, text: &str) -> TrackedDocument {
    let uri = tower_lsp::lsp_types::Url::from_file_path(format!("/proj/{name}")).unwrap();
    TrackedDocument::new(
        uri,
        1,
        text,
        GradleFileKind::RootBuildScript(DslLanguage::Groovy),
    )
}

/// Analyzes one script into a graph keyed by its file name (matching the server's keying).
fn graph_for(name: &str, text: &str, language: DslLanguage) -> (SemanticGraph, SemanticInput) {
    let kind = GradleFileKind::RootBuildScript(language);
    let input = SemanticInput::script(name, text, kind);
    let graph = analyze_documents(std::slice::from_ref(&input));
    (graph, input)
}

/// Computes the actions over the whole document for a freshly parsed script.
fn actions_over_all(doc: &TrackedDocument, parse: &Parse, language: DslLanguage) -> Vec<CodeActionModel> {
    let name = doc
        .uri()
        .to_file_path()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (graph, input) = graph_for(&name, doc.text(), language);
    let diags = compute_diagnostics(doc, parse, graph.document(&input.id).unwrap());
    code_actions(doc, parse, &graph, &diags, TextSpan::new(0, doc.text().len()))
}

#[test]
fn missing_brace_offered_on_single_eof_unclosed_block() {
    let text = "dependencies {\n    implementation(\"a:b:1.0\")\n";
    let doc = kotlin_doc("build.gradle.kts", text);
    let parse = parse_kotlin(text);

    let actions = actions_over_all(&doc, &parse, DslLanguage::Kotlin);
    let brace: Vec<_> = actions
        .iter()
        .filter(|a| a.title_key == MessageKey::CodeActionInsertClosingBrace)
        .collect();
    assert_eq!(brace.len(), 1, "exactly one brace fix on a single unclosed block");
    let edit = &brace[0].edits[0];
    assert_eq!(edit.new_text, "}");
    assert_eq!(edit.span.start, text.len(), "brace inserted at EOF");
    assert_eq!(edit.span.len, 0, "insertion is zero-width");
    assert_eq!(brace[0].category, CodeActionCategory::QuickFix);
}

#[test]
fn missing_brace_suppressed_on_multi_error_file() {
    // Two unclosed blocks => more than one syntax error => ambiguous => NO brace fix.
    let text = "dependencies {\n    implementation(\nplugins {\n    id(\n";
    let doc = kotlin_doc("build.gradle.kts", text);
    let parse = parse_kotlin(text);
    assert!(
        parse.errors.len() >= 2,
        "fixture must produce a multi-error parse (got {})",
        parse.errors.len()
    );

    let actions = actions_over_all(&doc, &parse, DslLanguage::Kotlin);
    assert!(
        !actions
            .iter()
            .any(|a| a.title_key == MessageKey::CodeActionInsertClosingBrace),
        "no brace fix in an ambiguous multi-error context"
    );
}

#[test]
fn well_formed_file_offers_no_brace_fix() {
    let text = "dependencies {\n    implementation(\"a:b:1.0\")\n}\n";
    let doc = kotlin_doc("build.gradle.kts", text);
    let parse = parse_kotlin(text);
    assert!(parse.errors.is_empty(), "fixture must parse cleanly");

    let actions = actions_over_all(&doc, &parse, DslLanguage::Kotlin);
    assert!(
        !actions
            .iter()
            .any(|a| a.title_key == MessageKey::CodeActionInsertClosingBrace)
    );
}

#[test]
fn unused_import_removal_deletes_exactly_the_import_line() {
    let text = "import a.b.Unused\ntask build {}\n";
    let doc = groovy_doc("build.gradle", text);
    let parse = parse_groovy(text);

    let actions = actions_over_all(&doc, &parse, DslLanguage::Groovy);
    let removal: Vec<_> = actions
        .iter()
        .filter(|a| a.title_key == MessageKey::CodeActionRemoveUnusedImport)
        .collect();
    assert_eq!(removal.len(), 1, "exactly one unused-import fix");
    let edit = &removal[0].edits[0];
    assert_eq!(edit.new_text, "", "removal is a deletion");
    // The edit deletes exactly the first line including its newline, nothing else.
    let deleted = edit.span.text(text);
    assert_eq!(deleted, "import a.b.Unused\n");
    // Applying the edit leaves the task declaration untouched.
    let mut applied = text.to_string();
    applied.replace_range(edit.span.start..edit.span.end(), "");
    assert_eq!(applied, "task build {}\n");
}

#[test]
fn duplicate_declaration_removal_offered_for_duplicate() {
    let text = "task build {}\ntask build {}\n";
    let doc = groovy_doc("build.gradle", text);
    let parse = parse_groovy(text);

    let actions = actions_over_all(&doc, &parse, DslLanguage::Groovy);
    let dup: Vec<_> = actions
        .iter()
        .filter(|a| a.title_key == MessageKey::CodeActionRemoveDuplicate)
        .collect();
    assert_eq!(dup.len(), 1, "the second `task build` is flagged once");
    assert_eq!(dup[0].edits[0].new_text, "");
    assert!(dup[0].edits[0].span.text(text).contains("build"));
}

#[test]
fn modernize_configuration_offered_for_deprecated_config() {
    let text = "dependencies {\n    compile 'a:b:1.0'\n}\n";
    let doc = groovy_doc("build.gradle", text);
    let parse = parse_groovy(text);

    let actions = actions_over_all(&doc, &parse, DslLanguage::Groovy);
    let modernize: Vec<_> = actions
        .iter()
        .filter(|a| a.title_key == MessageKey::CodeActionModernizeConfiguration)
        .collect();
    assert_eq!(modernize.len(), 1, "exactly one modernize action for `compile`");
    let action = modernize[0];
    assert_eq!(action.category, CodeActionCategory::Rewrite);
    assert_eq!(action.title_args, vec!["compile".to_string(), "implementation".to_string()]);
    let edit = &action.edits[0];
    assert_eq!(edit.new_text, "implementation");
    assert_eq!(edit.span.text(text), "compile", "replaces only the config head token");
}

#[test]
fn modern_configuration_not_offered_for_current_config() {
    let text = "dependencies {\n    implementation 'a:b:1.0'\n}\n";
    let doc = groovy_doc("build.gradle", text);
    let parse = parse_groovy(text);

    let actions = actions_over_all(&doc, &parse, DslLanguage::Groovy);
    assert!(
        !actions
            .iter()
            .any(|a| a.title_key == MessageKey::CodeActionModernizeConfiguration),
        "a modern configuration is left alone"
    );
}

#[test]
fn range_outside_a_diagnostic_offers_no_diagnostic_fix() {
    let text = "import a.b.Unused\ntask build {}\n";
    let doc = groovy_doc("build.gradle", text);
    let parse = parse_groovy(text);
    let (graph, input) = graph_for("build.gradle", text, DslLanguage::Groovy);
    let diags = compute_diagnostics(&doc, &parse, graph.document(&input.id).unwrap());

    // Request range over the `task build {}` line only — not over the import.
    let range = TextSpan::from_range(text.find("task").unwrap(), text.len());
    let actions = code_actions(&doc, &parse, &graph, &diags, range);
    assert!(
        !actions
            .iter()
            .any(|a| a.title_key == MessageKey::CodeActionRemoveUnusedImport),
        "the unused-import fix is not offered away from the import"
    );
}

#[test]
fn no_actions_for_non_dsl_document() {
    let text = "[versions]\nguava = \"33.0\"\n";
    let uri = tower_lsp::lsp_types::Url::from_file_path("/proj/gradle/libs.versions.toml").unwrap();
    let doc = TrackedDocument::new(uri, 1, text, GradleFileKind::VersionCatalog);
    let parse = parse_groovy(text);
    let graph = SemanticGraph::new();
    let actions = code_actions(&doc, &parse, &graph, &[], TextSpan::new(0, text.len()));
    assert!(actions.is_empty(), "a version catalog is not a DSL document");
}

#[test]
fn malformed_input_never_panics_and_stays_bounded() {
    let text = "}{ task ( dependencies @@@ \n plugins {{{ \n";
    let doc = groovy_doc("build.gradle", text);
    let parse = parse_groovy(text);
    // Just assert it returns (no panic); content is whatever the preconditions allow.
    let _ = actions_over_all(&doc, &parse, DslLanguage::Groovy);
}
