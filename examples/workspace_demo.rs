//! Manual-QA harness for the Task 2 workspace + document model.
//!
//! Builds a throwaway multi-project Gradle fixture in a temp directory, classifies each
//! file, resolves the workspace root, and prints the [`InvalidationScope`] that editing
//! each file would force — then self-cleans the temp tree. Run with:
//!
//! ```text
//! cargo run --example workspace_demo
//! ```
//!
//! Observable PASS: `app/build.gradle.kts` => `SubprojectBuildScript(Kotlin)` with root ==
//! the settings directory (NOT `app/`); `gradle/libs.versions.toml` => `VersionCatalog`
//! with a `WorkspaceSemantic` invalidation.

use std::fs;
use std::path::{Path, PathBuf};

use gradle_analyzer::gradle::workspace::{
    ChangeTrigger, GradleFileKind, WorkspaceDocumentStore, detect_workspace_root, invalidation_for,
};
use gradle_analyzer::i18n::Translator;
use tower_lsp::lsp_types::Url;

fn main() {
    let root = make_fixture();
    println!("== workspace_demo fixture root: {} ==\n", root.display());

    let translator = Translator::new();

    let files = [
        root.join("settings.gradle.kts"),
        root.join("app/build.gradle.kts"),
        root.join("gradle/libs.versions.toml"),
        root.join("buildSrc/build.gradle.kts"),
        root.join("gradle.properties"),
    ];

    for file in &files {
        report_file(&root, file, &translator);
    }

    teardown(&root);
    println!("\n== fixture torn down: {} (removed) ==", root.display());
}

/// Classifies, root-resolves, and prints the invalidation for one fixture file.
fn report_file(root: &Path, file: &Path, translator: &Translator) {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let kind = GradleFileKind::classify(file, root);
    let resolved = detect_workspace_root(file);
    let trigger = ChangeTrigger::for_path(file, root);
    let scope = invalidation_for(trigger);

    println!("file: {}", rel.display());
    println!("  kind            : {kind:?}");
    match &resolved {
        Some(r) => {
            println!("  root            : {}", r.path().display());
            println!("  root status     : {}", r.status_message(translator));
        }
        None => println!("  root            : <unresolved>"),
    }
    println!("  change trigger  : {trigger:?}");
    println!("  invalidation    : {:?}", scope.kind());
    println!(
        "    workspace_semantic={} sidecar_refresh={}",
        scope.needs_workspace_semantic(),
        scope.needs_sidecar_refresh()
    );

    // Exercise the store lifecycle on the file so the demo also proves open/change tracking.
    if let Ok(uri) = Url::from_file_path(file) {
        demo_store_lifecycle(uri);
    }
    println!();
}

/// Runs an open -> change -> close round on a fresh store for one URI.
fn demo_store_lifecycle(uri: Url) {
    let mut store = WorkspaceDocumentStore::new();
    let opened = store.open(uri.clone(), 1, "// v1\n");
    let changed = store
        .change(&uri, 2, "// v2 edited\n")
        .expect("change after open");
    println!(
        "  store lifecycle : open v{} -> change v{} (old snapshot still v{})",
        opened.version(),
        changed.version(),
        opened.version()
    );
    store.close(&uri);
}

/// Creates the throwaway multi-project fixture and returns its root path.
fn make_fixture() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ga-workspace-demo-{}-{}", std::process::id(), unique));

    write_file(&root.join("settings.gradle.kts"), "include(\":app\")\n");
    write_file(&root.join("app/build.gradle.kts"), "plugins { kotlin(\"jvm\") }\n");
    write_file(
        &root.join("gradle/libs.versions.toml"),
        "[versions]\nkotlin = \"2.0.0\"\n",
    );
    write_file(
        &root.join("buildSrc/build.gradle.kts"),
        "plugins { `kotlin-dsl` }\n",
    );
    write_file(&root.join("gradle.properties"), "org.gradle.jvmargs=-Xmx2g\n");
    root
}

/// Writes `contents` to `path`, creating parent directories as needed.
fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// Removes the fixture tree, ignoring an already-gone directory.
fn teardown(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
