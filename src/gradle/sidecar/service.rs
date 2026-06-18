//! [`SidecarService`]: on-demand model-import orchestration over the real child process.
//!
//! The service ties the pieces together: it acquires a per-workspace concurrency permit
//! ([`SidecarConfig::max_concurrent`]), plans the launch ([`plan_launch`]), spawns the real
//! [`WrapperRunner`], drives the [`SidecarClient`] handshake + model request (with the
//! config-backed timeout and an optional cancel token), and caches the result keyed by
//! Gradle version + classpath fingerprint. Every failure path degrades to the static tier
//! with a localized [`SidecarFailure`] — the service is never required at startup and never
//! blocks the static tier.
//!
//! All limits/timeouts come from [`ConfigManager`]; `tracing` wraps the import lifecycle.

use std::sync::{Arc, Mutex};

use tokio::sync::{Semaphore, oneshot};

use crate::config::ConfigManager;
use crate::gradle::sidecar::cache::{ModelCache, ModelCacheKey};
use crate::gradle::sidecar::client::SidecarClient;
use crate::gradle::sidecar::failure::SidecarFailure;
use crate::gradle::sidecar::launch::{LaunchInputs, plan_launch};
use crate::gradle::sidecar::model::SidecarModel;
use crate::gradle::sidecar::wrapper_runner::{CommandSpec, WrapperRunner};

/// Orchestrates on-demand Gradle model imports through the real JVM sidecar.
///
/// Cheap to clone (the held [`ConfigManager`] and shared cache/semaphore are `Arc`s).
/// `max_concurrent` is read once at construction to size the permit pool; a later config
/// reload changing it takes effect when a new service is built (documented limitation that
/// keeps the permit pool a fixed, predictable bound).
///
/// # Example
///
/// ```no_run
/// # async fn run() {
/// use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
/// use gradle_analyzer::gradle::sidecar::service::SidecarService;
/// use gradle_analyzer::gradle::sidecar::launch::LaunchInputs;
/// use std::path::PathBuf;
///
/// let service = SidecarService::new(ConfigManager::new(GradleAnalyzerConfig::default()));
/// let inputs = LaunchInputs::discover(
///     PathBuf::from("/path/to/project"),
///     PathBuf::from("/path/to/sidecar/classes"),
///     PathBuf::from("/path/to/sidecar-init.gradle"),
///     PathBuf::from("/tmp/ga-sidecar-out.json"),
/// );
/// match service.import(inputs, None).await {
///     Ok(model) => println!("gradle {}", model.gradle_version),
///     Err(failure) => assert!(failure.degraded_to_static()),
/// }
/// # }
/// ```
#[derive(Clone)]
pub struct SidecarService {
    config: ConfigManager,
    semaphore: Arc<Semaphore>,
    cache: Arc<Mutex<ModelCache>>,
}

impl SidecarService {
    /// Creates a service whose permit pool is sized by `config`'s `max_concurrent`.
    pub fn new(config: ConfigManager) -> Self {
        let max = config.snapshot().sidecar.max_concurrent.max(1) as usize;
        Self {
            config,
            semaphore: Arc::new(Semaphore::new(max)),
            cache: Arc::new(Mutex::new(ModelCache::new())),
        }
    }

    /// Imports the Gradle model for `inputs`, returning it or a degraded [`SidecarFailure`].
    ///
    /// Acquires a concurrency permit (bounding parallel syncs), plans + launches the
    /// sidecar, drives the protocol under the config timeout, and on success caches the
    /// model. `cancel` aborts an in-flight import to [`SidecarFailure::Canceled`].
    pub async fn import(
        &self,
        inputs: LaunchInputs,
        cancel: Option<oneshot::Receiver<()>>,
    ) -> Result<SidecarModel, SidecarFailure> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| SidecarFailure::SyncFailure {
                detail: "sidecar permit pool closed".to_string(),
            })?;

        let plan = plan_launch(&inputs)?;
        let spec = CommandSpec::from_plan(plan);
        let mut runner = WrapperRunner::new(spec);
        let client = SidecarClient::new(self.config.clone());

        tracing::info!(project = %inputs.project_dir.display(), "sidecar import starting");
        let result = match cancel {
            Some(rx) => client.import_model_cancelable(&mut runner, rx).await,
            None => client.import_model(&mut runner).await,
        };

        match &result {
            Ok(model) => {
                tracing::info!(
                    gradle = %model.gradle_version,
                    plugins = model.applied_plugins.len(),
                    extensions = model.extensions.len(),
                    "sidecar model imported"
                );
                self.store(model.clone());
            }
            Err(failure) => tracing::warn!(
                key = %failure.message_key(),
                "sidecar import degraded to static tier"
            ),
        }
        result
    }

    /// Returns the cached model for `key` iff it is fresh, else [`SidecarFailure::StaleCache`].
    ///
    /// This is the cache-only path the refinement layer (Task 16) uses to reuse a prior
    /// import without re-launching; a key mismatch (changed Gradle version or classpath) is
    /// reported as a stale degradation rather than a silent miss.
    pub fn cached_or_stale(&self, key: &ModelCacheKey) -> Result<SidecarModel, SidecarFailure> {
        let cache = self.cache.lock().expect("model cache mutex");
        match cache.get(key) {
            Some(model) => Ok(model.clone()),
            None => {
                tracing::info!("sidecar cache stale; advanced model unavailable until refresh");
                Err(SidecarFailure::StaleCache)
            }
        }
    }

    /// Stores `model` in the cache under its derived key (briefly locking, never across an await).
    fn store(&self, model: SidecarModel) {
        self.cache.lock().expect("model cache mutex").store(model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GradleAnalyzerConfig;
    use crate::gradle::sidecar::protocol::{Capability, ServerHello, ServerMessage};
    use crate::gradle::sidecar::runner::ProcessRunner;
    use std::path::{Path, PathBuf};

    /// A service with a tiny request timeout so hanging-child tests resolve fast.
    fn service_with_timeout(timeout_ms: u64) -> SidecarService {
        let mut config = GradleAnalyzerConfig::default();
        config.sidecar.request_timeout_ms = timeout_ms;
        SidecarService::new(ConfigManager::new(config))
    }

    /// Plan-valid inputs with present classes/init/gradle-home stubs, so `plan_launch`
    /// itself is NOT the thing under test (its own failures are covered in `launch.rs`).
    fn doubled_inputs(base: &Path) -> LaunchInputs {
        use std::fs;
        fs::create_dir_all(base.join("project")).unwrap();
        fs::create_dir_all(base.join("classes")).unwrap();
        fs::create_dir_all(base.join("gradle-home/lib")).unwrap();
        fs::write(base.join("gradle-home/lib/gradle-tooling-api-9.5.1.jar"), b"").unwrap();
        let java = base.join("java");
        fs::write(&java, b"#!/bin/sh\n").unwrap();
        let init = base.join("sidecar-init.gradle");
        fs::write(&init, b"// init\n").unwrap();
        LaunchInputs {
            project_dir: base.join("project"),
            gradle_home: Some(base.join("gradle-home")),
            classes_dir: base.join("classes"),
            init_script: init,
            java_exe: Some(java),
            out_file: base.join("out.json"),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ga-service-{}-{}-{}",
            tag,
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Resolves `sh` (used to script deterministic process doubles).
    fn sh() -> String {
        for c in ["/bin/sh", "/usr/bin/sh"] {
            if Path::new(c).exists() {
                return c.to_string();
            }
        }
        "sh".to_string()
    }

    /// Writes a valid `ServerHello` frame to a temp file the doubles can `cat`.
    fn hello_file(base: &Path) -> String {
        let hello = ServerMessage::Hello(ServerHello {
            chosen_version: 1,
            capabilities: vec![Capability::ModelImport, Capability::Cancellation],
        });
        let line = serde_json::to_string(&hello).unwrap();
        let path = base.join("hello.json");
        std::fs::write(&path, format!("{line}\n")).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// Drives the real `WrapperRunner` against a scripted `sh -c` double.
    async fn drive(
        service: &SidecarService,
        script: String,
        cancel: Option<oneshot::Receiver<()>>,
    ) -> Result<SidecarModel, SidecarFailure> {
        let spec = CommandSpec::new(sh(), vec!["-c".to_string(), script]);
        let mut runner = WrapperRunner::new(spec);
        let client = SidecarClient::new(service.config.clone());
        let result = match cancel {
            Some(rx) => client.import_model_cancelable(&mut runner, rx).await,
            None => client.import_model(&mut runner).await,
        };
        let _ = runner.kill().await;
        result
    }

    // --- plan-level degradations exercised through the full service.import path ---

    #[tokio::test]
    async fn missing_jvm_degrades_to_static_without_panicking() {
        let base = temp_dir("nojvm");
        let mut inputs = doubled_inputs(&base);
        inputs.java_exe = None;

        let service = service_with_timeout(30_000);
        let failure = service.import(inputs, None).await.unwrap_err();
        assert_eq!(failure, SidecarFailure::MissingJvm);
        assert!(failure.degraded_to_static());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn missing_wrapper_and_installation_degrades() {
        let base = temp_dir("nowrap");
        let mut inputs = doubled_inputs(&base);
        inputs.gradle_home = None;

        let service = service_with_timeout(30_000);
        let failure = service.import(inputs, None).await.unwrap_err();
        assert!(failure.degraded_to_static());
        std::fs::remove_dir_all(&base).unwrap();
    }

    // --- transport-level degradations against real (non-JVM) process doubles ---

    #[tokio::test(start_paused = true)]
    async fn hanging_child_times_out_and_degrades() {
        // No reply at all: the handshake read hits the config deadline.
        let service = service_with_timeout(50);
        let failure = drive(&service, "sleep 60".to_string(), None).await.unwrap_err();
        assert_eq!(failure, SidecarFailure::Timeout { elapsed_ms: 50 });
        assert!(failure.degraded_to_static());
    }

    #[tokio::test]
    async fn malformed_child_output_degrades_to_malformed_frame() {
        // A non-JSON first line -> MalformedFrame at the handshake decode.
        let service = service_with_timeout(30_000);
        let failure = drive(&service, "printf 'not-json\\n'; sleep 1".to_string(), None)
            .await
            .unwrap_err();
        assert!(matches!(failure, SidecarFailure::MalformedFrame { .. }));
        assert!(failure.degraded_to_static());
    }

    #[tokio::test]
    async fn early_exit_before_reply_degrades_to_sync_failure() {
        // Child exits immediately with no output -> clean EOF at handshake.
        let service = service_with_timeout(30_000);
        let failure = drive(&service, "exit 0".to_string(), None).await.unwrap_err();
        assert!(matches!(failure, SidecarFailure::SyncFailure { .. }));
        assert!(failure.degraded_to_static());
    }

    #[tokio::test]
    async fn canceled_import_resolves_to_canceled() {
        // Valid hello, then hang on the model request -> cancel wins.
        let base = temp_dir("cancel");
        let service = service_with_timeout(30_000);
        let hello = hello_file(&base);
        let script = format!("cat {hello}; sleep 60");

        let (tx, rx) = oneshot::channel();
        tx.send(()).unwrap();
        let failure = drive(&service, script, Some(rx)).await.unwrap_err();
        assert_eq!(failure, SidecarFailure::Canceled);
        assert!(failure.degraded_to_static());
        std::fs::remove_dir_all(&base).unwrap();
    }

    // --- stale-cache contract ---

    #[test]
    fn cached_or_stale_reports_stale_on_miss_and_hit_after_store() {
        let service = service_with_timeout(30_000);
        let model = SidecarModel {
            gradle_version: "9.5.1".to_string(),
            classpath_jars: vec!["a.jar".to_string()],
            ..SidecarModel::default()
        };
        let key = ModelCacheKey::of_model(&model);
        assert_eq!(
            service.cached_or_stale(&key).unwrap_err(),
            SidecarFailure::StaleCache
        );

        service.store(model);
        assert_eq!(service.cached_or_stale(&key).unwrap().gradle_version, "9.5.1");

        let changed = ModelCacheKey::new("9.5.1", &["a.jar".into(), "b.jar".into()]);
        assert_eq!(
            service.cached_or_stale(&changed).unwrap_err(),
            SidecarFailure::StaleCache
        );
    }
}
