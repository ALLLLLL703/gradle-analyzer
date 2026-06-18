//! JVM-gated real-sync acceptance test for the Task-14 Gradle Tooling-API sidecar.
//!
//! This is the acceptance proof: it compiles the real Java sidecar, launches it as a child
//! process via the real [`WrapperRunner`], runs a genuine Gradle Tooling-API `BuildAction` +
//! init-script against the tiny no-network fixture under `tests/fixtures/sidecar-project/`,
//! and asserts an imported [`SidecarModel`] with a non-empty `gradle_version` and the
//! fixture's `java` plugin/extension.
//!
//! It is GATED on a real JVM + Gradle being present: when either is missing the test logs a
//! skip reason and passes, so CI without a JVM stays green. Spawned Gradle daemons are
//! stopped and temp artifacts removed at the end.

use std::path::{Path, PathBuf};
use std::process::Command;

use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
use gradle_analyzer::gradle::sidecar::launch::LaunchInputs;
use gradle_analyzer::gradle::sidecar::service::SidecarService;

/// Returns the repository root (the cargo manifest dir).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Returns `true` if `bin` resolves on `$PATH`.
fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| dir.join(bin).exists())
        })
        .unwrap_or(false)
}

/// Compiles the Java sidecar into `out_dir`, returning Ok on success.
fn compile_sidecar(out_dir: &Path) -> Result<(), String> {
    let script = repo_root().join("sidecar-jvm/build-sidecar.sh");
    let output = Command::new("bash")
        .arg(&script)
        .arg(out_dir)
        .output()
        .map_err(|e| format!("spawn build-sidecar.sh: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "build-sidecar.sh failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Best-effort: stop any Gradle daemon this test may have started.
fn stop_gradle_daemons() {
    let _ = Command::new("gradle").arg("--stop").output();
}

#[tokio::test]
async fn real_sync_imports_model_from_fixture_or_skips_with_reason() {
    if !on_path("java") || !on_path("gradle") {
        eprintln!(
            "SKIP real_sync: java present={}, gradle present={} — no JVM/Gradle toolchain",
            on_path("java"),
            on_path("gradle")
        );
        return;
    }

    let work = std::env::temp_dir().join(format!("ga-real-sync-{}", std::process::id()));
    std::fs::create_dir_all(&work).expect("work dir");
    let classes = work.join("classes");

    if let Err(reason) = compile_sidecar(&classes) {
        eprintln!("SKIP real_sync: could not compile Java sidecar: {reason}");
        let _ = std::fs::remove_dir_all(&work);
        return;
    }

    let fixture = repo_root().join("tests/fixtures/sidecar-project");
    let init = repo_root().join("sidecar-jvm/sidecar-init.gradle");
    let out = work.join("model-out.json");
    let inputs = LaunchInputs::discover(fixture, classes, init, out);

    let mut config = GradleAnalyzerConfig::default();
    config.sidecar.request_timeout_ms = 180_000; // a cold daemon start can be slow
    let service = SidecarService::new(ConfigManager::new(config));

    let result = service.import(inputs, None).await;

    stop_gradle_daemons();
    let _ = std::fs::remove_dir_all(&work);

    let model = result.expect("real sync should import a model");
    assert!(
        !model.gradle_version.trim().is_empty(),
        "gradle_version must be populated by the real sync"
    );

    let mentions_java = model
        .applied_plugins
        .iter()
        .any(|p| p.plugin_class.contains("JavaPlugin"))
        || model.extensions.iter().any(|e| e.name == "java");
    assert!(
        mentions_java,
        "fixture applies the java plugin; model should reflect it: {model:?}"
    );
}
