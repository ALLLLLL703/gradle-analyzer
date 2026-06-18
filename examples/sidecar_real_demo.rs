//! Manual-QA demo for the **real** Task-14 JVM Gradle Tooling-API sidecar.
//!
//! Unlike `sidecar_fake_demo` (a scripted [`FakeRunner`], no JVM), this drives the real
//! [`SidecarService`] against a real Gradle build via a real child process. Run with:
//!
//! ```text
//! cargo run --example sidecar_real_demo -- <project-dir> <classes-dir> <init-script> [gradle-home]
//! ```
//!
//! It prints two scenarios:
//!
//! 1. **Real import** — launches the JVM sidecar against `<project-dir>`, handshakes, runs
//!    the Tooling-API `BuildAction` + init-script, and prints the imported [`SidecarModel`]
//!    (gradle version, applied plugins, extension DSL blocks).
//! 2. **Degradation** — points the runner at a directory with no wrapper and no
//!    installation, and prints the localized [`SidecarFailure`] + `degraded_to_static=true`.
//!
//! With no args it runs ONLY the degradation scenario (no JVM needed), so the example is
//! always runnable.

use std::path::PathBuf;

use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
use gradle_analyzer::gradle::sidecar::launch::LaunchInputs;
use gradle_analyzer::gradle::sidecar::service::SidecarService;
use gradle_analyzer::i18n::{MessageKey, Translator};

#[tokio::main]
async fn main() {
    let translator = Translator::new();
    println!("=== gradle-analyzer REAL sidecar demo ===\n");

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() >= 3 {
        real_import(&translator, &args).await;
        println!();
    } else {
        println!("(no <project> <classes> <init> args given; running degradation only)\n");
    }

    degradation(&translator).await;
}

/// Scenario 1: a real model import through the JVM sidecar.
async fn real_import(translator: &Translator, args: &[String]) {
    println!("--- Scenario 1: real Gradle model import ---");
    let project = PathBuf::from(&args[0]);
    let classes = PathBuf::from(&args[1]);
    let init = PathBuf::from(&args[2]);
    let out = std::env::temp_dir().join("ga-sidecar-demo-out.json");

    let mut inputs = LaunchInputs::discover(project, classes, init, out);
    if let Some(home) = args.get(3) {
        inputs.gradle_home = Some(PathBuf::from(home));
    }

    let service = SidecarService::new(ConfigManager::new(GradleAnalyzerConfig::default()));
    match service.import(inputs, None).await {
        Ok(model) => {
            let status = translator.get_text(
                MessageKey::SidecarModelImported,
                &[
                    &model.gradle_version,
                    &model.applied_plugins.len().to_string(),
                    &model.extensions.len().to_string(),
                ],
            );
            println!("{status}");
            println!("gradle_version = {}", model.gradle_version);
            for plugin in &model.applied_plugins {
                println!("  plugin: {}", plugin.plugin_class);
            }
            for ext in &model.extensions {
                println!("  extension: {} -> {}", ext.name, ext.type_fqn);
            }
            println!("classpath_jars = {}", model.classpath_jars.len());
        }
        Err(failure) => println!("import failed: {failure:?} (degraded_to_static={})", failure.degraded_to_static()),
    }
}

/// Scenario 2: a clean degraded fallback with a localized status.
async fn degradation(translator: &Translator) {
    println!("--- Scenario 2: degradation (no wrapper, no installation) ---");
    let inputs = LaunchInputs {
        project_dir: std::env::temp_dir().join("ga-sidecar-no-gradle"),
        gradle_home: None,
        classes_dir: std::env::temp_dir().join("ga-sidecar-no-classes"),
        init_script: std::env::temp_dir().join("ga-sidecar-no-init.gradle"),
        java_exe: None,
        out_file: std::env::temp_dir().join("ga-sidecar-no-out.json"),
    };
    std::fs::create_dir_all(&inputs.project_dir).ok();

    let service = SidecarService::new(ConfigManager::new(GradleAnalyzerConfig::default()));
    match service.import(inputs, None).await {
        Ok(model) => println!("UNEXPECTED success: {model:?}"),
        Err(failure) => {
            println!("failure = {failure:?}");
            println!("degraded_to_static = {}", failure.degraded_to_static());
            println!("localized status = {}", failure.status_message(translator));
            println!(
                "PASS: degraded cleanly = {}",
                failure.degraded_to_static() && !failure.status_message(translator).trim().is_empty()
            );
        }
    }
}
