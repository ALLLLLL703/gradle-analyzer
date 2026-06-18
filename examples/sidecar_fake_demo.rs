//! Manual-QA demo for the Task 4 sidecar IPC contract, driven entirely by a scripted
//! [`FakeRunner`] (no JVM, no real process).
//!
//! Run with:
//!
//! ```text
//! cargo run --example sidecar_fake_demo
//! ```
//!
//! It prints two scenarios over the real public API:
//!
//! 1. **Happy import** — handshake (version + capability negotiation) then a parsed
//!    [`SidecarModel`], including the `dotnet {}` plugin extension block.
//! 2. **Timeout degradation** — a never-replying sidecar hits the config-backed deadline
//!    and degrades to the static tier, printing `Timeout`, `degraded_to_static=true`, and
//!    the localized status string.

use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
use gradle_analyzer::gradle::sidecar::model::{AppliedPlugin, ExtensionInfo, SidecarModel};
use gradle_analyzer::gradle::sidecar::protocol::{
    Capability, ResponseOutcome, ServerHello, SidecarResponse,
};
use gradle_analyzer::gradle::sidecar::{FakeRunner, SidecarClient, SidecarFailure};
use gradle_analyzer::i18n::Translator;

#[tokio::main]
async fn main() {
    let translator = Translator::new();

    println!("=== gradle-analyzer sidecar IPC demo (FakeRunner, no JVM) ===\n");

    happy_import().await;
    println!();
    timeout_degradation(&translator).await;
}

/// Scenario 1: handshake + a successful model import carrying the `dotnet` extension.
async fn happy_import() {
    println!("--- Scenario 1: handshake -> happy model import ---");

    let client = SidecarClient::new(ConfigManager::new(GradleAnalyzerConfig::default()));
    let mut runner = FakeRunner::builder()
        .hello(ServerHello {
            chosen_version: 1,
            capabilities: vec![Capability::ModelImport, Capability::Cancellation],
        })
        .response(SidecarResponse {
            id: 1,
            outcome: ResponseOutcome::Model(demo_model()),
        })
        .build();

    match client.import_model(&mut runner).await {
        Ok(model) => {
            println!("imported model: gradle_version = {}", model.gradle_version);
            println!("applied_plugins = {}", model.applied_plugins.len());
            for ext in &model.extensions {
                println!("  extension: {} -> {}", ext.name, ext.type_fqn);
            }
            let dotnet = model.extensions.iter().find(|e| e.name == "dotnet");
            println!("PASS: dotnet extension present = {}", dotnet.is_some());
        }
        Err(failure) => println!("UNEXPECTED failure: {failure:?}"),
    }

    println!("wire frames the client wrote:");
    for line in runner.written() {
        println!("  -> {line}");
    }
}

/// Scenario 2: a never-replying sidecar times out and degrades to the static tier.
async fn timeout_degradation(translator: &Translator) {
    println!("--- Scenario 2: timeout -> degrade to static ---");

    // A deliberately tiny config deadline keeps the demo fast without hardcoding it in
    // library code; the client reads it from the snapshot.
    let mut config = GradleAnalyzerConfig::default();
    config.sidecar.request_timeout_ms = 100;
    let client = SidecarClient::new(ConfigManager::new(config));

    let mut runner = FakeRunner::builder()
        .hello(ServerHello {
            chosen_version: 1,
            capabilities: vec![Capability::ModelImport],
        })
        .hang()
        .build();

    match client.import_model(&mut runner).await {
        Ok(model) => println!("UNEXPECTED success: {model:?}"),
        Err(failure) => {
            let is_timeout = matches!(failure, SidecarFailure::Timeout { .. });
            println!("failure = {failure:?}");
            println!("is Timeout = {is_timeout}");
            println!("degraded_to_static = {}", failure.degraded_to_static());
            println!("localized status = {}", failure.status_message(translator));
            println!(
                "PASS: timeout degraded to static = {}",
                is_timeout && failure.degraded_to_static()
            );
        }
    }
}

/// Builds a representative model with applied plugins and a `dotnet {}` extension.
fn demo_model() -> SidecarModel {
    SidecarModel {
        gradle_version: "8.10".to_string(),
        applied_plugins: vec![AppliedPlugin {
            id: "org.jetbrains.kotlin.jvm".to_string(),
            plugin_class: "org.jetbrains.kotlin.gradle.plugin.KotlinPluginWrapper".to_string(),
        }],
        extensions: vec![ExtensionInfo {
            name: "dotnet".to_string(),
            type_fqn: "com.example.gradle.DotnetExtension".to_string(),
        }],
        classpath_jars: vec!["/repo/.gradle/kotlin-stdlib.jar".to_string()],
        ..SidecarModel::default()
    }
}

/// The demo doubles as a smoke test so `cargo test` exercises both scenarios.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn demo_scenarios_run_without_panicking() {
        let translator = Translator::new();
        happy_import().await;

        let mut config = GradleAnalyzerConfig::default();
        config.sidecar.request_timeout_ms = 20;
        let client = SidecarClient::new(ConfigManager::new(config));
        let mut runner = FakeRunner::builder()
            .hello(ServerHello {
                chosen_version: 1,
                capabilities: vec![Capability::ModelImport],
            })
            .hang()
            .build();
        let failure = client.import_model(&mut runner).await.unwrap_err();
        assert!(matches!(failure, SidecarFailure::Timeout { .. }));
        assert!(failure.degraded_to_static());
        assert!(!failure.status_message(&translator).trim().is_empty());
    }
}
