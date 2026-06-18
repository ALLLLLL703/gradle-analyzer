//! Tests for the document-symbol outline: both DSLs, partial input, ranges, incremental.
//!
//! These assert the LSP-type-free [`SymbolNode`] tree (the feature core) plus a boundary
//! conversion sanity check. Sources are inline so the cases stay self-contained and do not
//! couple to fixtures owned by sibling tasks.

use tower_lsp::lsp_types::Url;

use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::SemanticGraph;
use crate::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};

use super::node::{OutlineKind, SymbolNode};
use super::{document_symbols, outline_lsp};

const KOTLIN_BUILD: &str = "\
plugins {
    id(\"java\")
    kotlin(\"jvm\") version \"1.9.22\"
}
repositories {
    mavenCentral()
}
dependencies {
    implementation(\"org.apache:commons:3.0\")
    implementation(libs.guava)
}
tasks.register(\"integ\") { }
group = \"com.example\"
";

const GROOVY_BUILD: &str = "\
plugins {
    id 'application'
}
repositories {
    google()
}
dependencies {
    implementation 'org.apache:commons:3.0'
}
task hello { }
";

const KOTLIN_UNCLOSED: &str = "\
plugins {
    id(\"java\")
}
repositories {
    mavenCentral()
}
dependencies {
    implementation(\"g:a:v\")
";

const GROOVY_UNCLOSED: &str = "\
plugins {
    id 'java'
}
dependencies {
    implementation 'g:a:v'
";

fn kotlin_doc(source: &str) -> TrackedDocument {
    let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
    TrackedDocument::new(uri, 1, source, GradleFileKind::RootBuildScript(DslLanguage::Kotlin))
}

fn groovy_doc(source: &str) -> TrackedDocument {
    let uri = Url::from_file_path("/proj/build.gradle").unwrap();
    TrackedDocument::new(uri, 1, source, GradleFileKind::RootBuildScript(DslLanguage::Groovy))
}

fn kotlin_symbols(source: &str) -> Vec<SymbolNode> {
    let doc = kotlin_doc(source);
    document_symbols(&doc, &parse_kotlin(source), &SemanticGraph::new())
}

fn groovy_symbols(source: &str) -> Vec<SymbolNode> {
    let doc = groovy_doc(source);
    document_symbols(&doc, &parse_groovy(source), &SemanticGraph::new())
}

/// Finds a top-level symbol by name.
fn find<'a>(symbols: &'a [SymbolNode], name: &str) -> Option<&'a SymbolNode> {
    symbols.iter().find(|s| s.name == name)
}

#[test]
fn kotlin_valid_script_yields_expected_hierarchy_and_kinds() {
    let symbols = kotlin_symbols(KOTLIN_BUILD);

    let plugins = find(&symbols, "plugins").expect("plugins block present");
    assert_eq!(plugins.kind, OutlineKind::Block);
    // Individual plugin ids surface as children with the coordinate as the name.
    let plugin_names: Vec<&str> = plugins.children.iter().map(|c| c.name.as_str()).collect();
    assert!(plugin_names.contains(&"java"), "plugin ids: {plugin_names:?}");
    assert!(
        plugins.children.iter().all(|c| c.kind == OutlineKind::Plugin),
        "all plugin children are Plugin kind"
    );

    let repositories = find(&symbols, "repositories").expect("repositories block present");
    assert_eq!(repositories.children[0].name, "mavenCentral");
    assert_eq!(repositories.children[0].kind, OutlineKind::Repository);

    let dependencies = find(&symbols, "dependencies").expect("dependencies block present");
    let dep = &dependencies.children[0];
    assert_eq!(dep.kind, OutlineKind::Dependency);
    assert_eq!(dep.name, "implementation", "configuration is the name");
    assert_eq!(dep.detail.as_deref(), Some("org.apache:commons:3.0"));

    // Nested-call task registration surfaces as a task symbol.
    let task = find(&symbols, "integ").expect("tasks.register surfaces a task");
    assert_eq!(task.kind, OutlineKind::Task);

    // A group assignment becomes a property with its value as the detail.
    let group = find(&symbols, "group").expect("group property present");
    assert_eq!(group.kind, OutlineKind::Property);
    assert_eq!(group.detail.as_deref(), Some("com.example"));
}

#[test]
fn groovy_valid_script_yields_expected_hierarchy_and_kinds() {
    let symbols = groovy_symbols(GROOVY_BUILD);

    let plugins = find(&symbols, "plugins").expect("plugins block present");
    assert!(
        plugins.children.iter().any(|c| c.name == "application" && c.kind == OutlineKind::Plugin),
        "groovy plugin id surfaces: {:?}",
        plugins.children
    );

    let repositories = find(&symbols, "repositories").expect("repositories block present");
    assert_eq!(repositories.children[0].name, "google");

    let dependencies = find(&symbols, "dependencies").expect("dependencies block present");
    let dep = &dependencies.children[0];
    assert_eq!(dep.name, "implementation");
    assert_eq!(dep.detail.as_deref(), Some("org.apache:commons:3.0"));

    // Groovy `task foo {}` registration surfaces as a task symbol.
    let task = find(&symbols, "hello").expect("groovy task surfaces");
    assert_eq!(task.kind, OutlineKind::Task);
}

#[test]
fn kotlin_partial_unclosed_dependencies_still_yields_early_symbols() {
    let symbols = kotlin_symbols(KOTLIN_UNCLOSED);
    // Early top-level symbols survive an unclosed later block.
    assert!(!symbols.is_empty(), "partial outline must be non-empty");
    assert!(find(&symbols, "plugins").is_some(), "early plugins block intact");
    assert!(find(&symbols, "repositories").is_some(), "early repositories block intact");
    // The dependency may live under the unclosed `dependencies` block; assert it is reachable
    // somewhere without asserting exact top-level recovery.
    assert!(
        contains_dependency(&symbols),
        "the implementation dependency is reachable in the partial outline: {symbols:?}"
    );
}

#[test]
fn groovy_partial_unclosed_dependencies_still_yields_early_symbols() {
    let symbols = groovy_symbols(GROOVY_UNCLOSED);
    assert!(!symbols.is_empty(), "partial outline must be non-empty");
    assert!(find(&symbols, "plugins").is_some(), "early plugins block intact");
    assert!(contains_dependency(&symbols), "dependency reachable: {symbols:?}");
}

/// Returns true if any node in the tree is a dependency symbol.
fn contains_dependency(symbols: &[SymbolNode]) -> bool {
    symbols.iter().any(|s| {
        s.kind == OutlineKind::Dependency || contains_dependency(&s.children)
    })
}

#[test]
fn ranges_are_correct_via_line_index() {
    // `plugins` starts at byte 0, line 0. `dependencies` starts after the repositories block.
    let symbols = kotlin_symbols(KOTLIN_BUILD);
    let plugins = find(&symbols, "plugins").unwrap();
    assert_eq!(plugins.span.start, 0, "plugins begins at byte 0");

    // Convert to LSP and check the plugins range starts at line 0, char 0.
    let doc = kotlin_doc(KOTLIN_BUILD);
    let lsp = outline_lsp(&doc);
    let plugins_lsp = lsp.iter().find(|s| s.name == "plugins").unwrap();
    assert_eq!(plugins_lsp.range.start.line, 0);
    assert_eq!(plugins_lsp.range.start.character, 0);
    // selection_range must be contained within range.
    assert!(plugins_lsp.selection_range.start.line >= plugins_lsp.range.start.line);

    // `dependencies` starts at the beginning of its own line.
    let deps_lsp = lsp.iter().find(|s| s.name == "dependencies").unwrap();
    let line_start_byte = KOTLIN_BUILD
        .lines()
        .take(deps_lsp.range.start.line as usize)
        .map(|l| l.len() + 1)
        .sum::<usize>();
    assert_eq!(
        deps_lsp.range.start.character, 0,
        "dependencies begins at column 0 (byte {line_start_byte})"
    );
}

#[test]
fn incremental_edit_keeps_outline_stable_and_correct() {
    // Baseline outline.
    let before = kotlin_symbols(KOTLIN_BUILD);
    let before_deps = find(&before, "dependencies").unwrap().children.len();

    // A small edit: add one dependency line inside the dependencies block.
    let edited = KOTLIN_BUILD.replace(
        "    implementation(libs.guava)\n",
        "    implementation(libs.guava)\n    api(\"x:y:1\")\n",
    );
    let after = kotlin_symbols(&edited);

    // Structure stays stable: same top-level blocks, dependencies grew by exactly one.
    assert_eq!(
        before.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
        after.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
        "top-level structure stable across the edit"
    );
    let after_deps = find(&after, "dependencies").unwrap();
    assert_eq!(after_deps.children.len(), before_deps + 1, "one dependency added");
    assert!(
        after_deps.children.iter().any(|c| c.name == "api" && c.detail.as_deref() == Some("x:y:1")),
        "the added dependency is present with its coordinate"
    );
}

#[test]
fn empty_and_garbage_input_never_panics() {
    for source in ["", "{{{", "plugins {", "= = =", "task"] {
        let _ = kotlin_symbols(source);
        let _ = groovy_symbols(source);
    }
}

#[test]
fn semantic_refinement_appends_resolved_catalog_coordinate() {
    use crate::gradle::semantic::{SemanticInput, analyze_documents};

    const CATALOG: &str = "\
[versions]
guava = \"33.0.0-jre\"

[libraries]
guava = { module = \"com.google.guava:guava\", version.ref = \"guava\" }
";
    const BUILD: &str = "\
dependencies {
    implementation(libs.guava)
}
";
    let graph = analyze_documents(&[
        SemanticInput::script("gradle/libs.versions.toml", CATALOG, GradleFileKind::VersionCatalog),
        SemanticInput::script(
            "build.gradle.kts",
            BUILD,
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
        ),
    ]);
    let doc = kotlin_doc(BUILD);
    let symbols = document_symbols(&doc, &parse_kotlin(BUILD), &graph);
    let deps = find(&symbols, "dependencies").unwrap();
    let dep = &deps.children[0];
    assert!(
        dep.detail.as_deref().is_some_and(|d| d.contains("com.google.guava:guava")),
        "resolved catalog coordinate appended to detail: {:?}",
        dep.detail
    );
}

#[test]
fn version_catalog_file_outlines_toml_sections() {
    const CATALOG: &str = "\
[versions]
guava = \"33.0\"

[libraries]
guava = { module = \"com.google.guava:guava\" }
";
    let uri = Url::from_file_path("/proj/gradle/libs.versions.toml").unwrap();
    let doc = TrackedDocument::new(uri, 1, CATALOG, GradleFileKind::VersionCatalog);
    // The catalog path ignores the parse, so any parse is fine here.
    let symbols = document_symbols(&doc, &parse_kotlin(""), &SemanticGraph::new());

    let versions = find(&symbols, "versions").expect("[versions] section present");
    assert_eq!(versions.kind, OutlineKind::Block);
    assert!(
        versions.children.iter().any(|c| c.name == "guava" && c.kind == OutlineKind::Property),
        "version key nested under its table: {:?}",
        versions.children
    );
    assert!(find(&symbols, "libraries").is_some(), "[libraries] section present");
}
