//! The [`SidecarClient`]: drives the handshake + a request/response round-trip over a
//! [`ProcessRunner`], enforcing the config-backed timeout and always degrading to static.
//!
//! The client owns no IO of its own — it is generic over [`ProcessRunner`], so the entire
//! protocol (handshake, version + capability negotiation, model request, cancellation,
//! timeout, malformed-frame recovery) is exercised against a scripted
//! [`crate::gradle::sidecar::runner::FakeRunner`] with no JVM. Every error path resolves to
//! a [`SidecarFailure`] (never a panic); the static tier stays live regardless.
//!
//! Timeouts and the max message size come from [`ConfigManager`] at call time, so a config
//! hot-reload takes effect on the next request without restarting anything.

use std::time::Duration;

use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::config::ConfigManager;
use crate::gradle::sidecar::failure::SidecarFailure;
use crate::gradle::sidecar::framing::{decode_line, to_line};
use crate::gradle::sidecar::model::SidecarModel;
use crate::gradle::sidecar::protocol::{
    Capability, CancelRequest, ClientHello, ClientMessage, NegotiatedSession, RequestAction,
    ResponseOutcome, ServerMessage, SidecarRequest, negotiate_capabilities, negotiate_version,
};
use crate::gradle::sidecar::runner::ProcessRunner;

/// The correlation id used for the single model-import request of a session.
const MODEL_REQUEST_ID: u64 = 1;

/// Orchestrates a sidecar session: handshake, model request, timeout, and cancellation.
///
/// Clone is cheap (the held [`ConfigManager`] is an `Arc` bump). The client reads the
/// timeout and size limit from the live config snapshot on each call.
///
/// # Example
///
/// ```
/// # async fn run() {
/// use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
/// use gradle_analyzer::gradle::sidecar::{
///     SidecarClient, FakeRunner,
///     protocol::{Capability, ServerHello, SidecarResponse, ResponseOutcome},
///     model::SidecarModel,
/// };
///
/// let client = SidecarClient::new(ConfigManager::new(GradleAnalyzerConfig::default()));
/// let mut runner = FakeRunner::builder()
///     .hello(ServerHello { chosen_version: 1, capabilities: vec![Capability::ModelImport] })
///     .response(SidecarResponse { id: 1, outcome: ResponseOutcome::Model(SidecarModel::default()) })
///     .build();
///
/// let model = client.import_model(&mut runner).await.expect("model");
/// assert_eq!(model.gradle_version, "");
/// # }
/// ```
#[derive(Clone)]
pub struct SidecarClient {
    config: ConfigManager,
}

impl SidecarClient {
    /// Creates a client that reads its timeout and size limits from `config`.
    pub fn new(config: ConfigManager) -> Self {
        Self { config }
    }

    /// The capabilities this client advertises, in preference order.
    fn requested_capabilities() -> Vec<Capability> {
        vec![
            Capability::ModelImport,
            Capability::Cancellation,
            Capability::SourceJars,
        ]
    }

    /// Runs the full session (handshake + model request), returning the model or a failure.
    ///
    /// Equivalent to [`SidecarClient::import_model_cancelable`] with a token that is never
    /// fired.
    pub async fn import_model<R: ProcessRunner>(
        &self,
        runner: &mut R,
    ) -> Result<SidecarModel, SidecarFailure> {
        self.run_session(runner, None).await
    }

    /// Runs the full session, aborting promptly to [`SidecarFailure::Canceled`] if `cancel`
    /// fires before the response arrives.
    ///
    /// A dropped sender (never fired) is treated as "no cancellation": the request proceeds
    /// under its normal timeout.
    pub async fn import_model_cancelable<R: ProcessRunner>(
        &self,
        runner: &mut R,
        cancel: oneshot::Receiver<()>,
    ) -> Result<SidecarModel, SidecarFailure> {
        self.run_session(runner, Some(cancel)).await
    }

    /// Shared session driver: spawn, handshake, request, then read the (cancelable) reply.
    async fn run_session<R: ProcessRunner>(
        &self,
        runner: &mut R,
        cancel: Option<oneshot::Receiver<()>>,
    ) -> Result<SidecarModel, SidecarFailure> {
        let snapshot = self.config.snapshot();
        let timeout_ms = snapshot.sidecar.request_timeout_ms;
        let max_bytes = snapshot.transport.max_message_bytes;

        runner
            .spawn()
            .await
            .map_err(|e| SidecarFailure::SyncFailure {
                detail: e.to_string(),
            })?;

        let session = self.handshake(runner, timeout_ms, max_bytes).await?;
        info!(
            version = session.version,
            capabilities = session.capabilities.len(),
            "sidecar handshake negotiated"
        );

        self.send_request(runner, max_bytes).await?;
        let message = self
            .read_message(runner, timeout_ms, max_bytes, cancel)
            .await
            .inspect_err(|failure| self.log_degradation("request", failure))?;

        self.expect_model(runner, message).await
    }

    /// Sends the [`ClientHello`] and validates the [`ServerHello`] version + capabilities.
    async fn handshake<R: ProcessRunner>(
        &self,
        runner: &mut R,
        timeout_ms: u64,
        max_bytes: usize,
    ) -> Result<NegotiatedSession, SidecarFailure> {
        let client_hello = ClientHello::current(Self::requested_capabilities());
        info!("sidecar handshake: sending client hello");
        self.send(runner, &ClientMessage::Hello(client_hello.clone()), max_bytes)
            .await?;

        let message = self
            .read_message(runner, timeout_ms, max_bytes, None)
            .await
            .inspect_err(|failure| self.log_degradation("handshake", failure))?;

        let server_hello = match message {
            ServerMessage::Hello(hello) => hello,
            ServerMessage::Response(_) => {
                let failure = SidecarFailure::MalformedFrame {
                    detail: "expected-hello".to_string(),
                };
                self.log_degradation("handshake", &failure);
                return Err(failure);
            }
        };

        let version = negotiate_version(&client_hello, &server_hello).ok_or_else(|| {
            let failure = SidecarFailure::SchemaMismatch {
                version: server_hello.chosen_version,
            };
            self.log_degradation("handshake", &failure);
            failure
        })?;

        Ok(NegotiatedSession {
            version,
            capabilities: negotiate_capabilities(
                &client_hello.capabilities,
                &server_hello.capabilities,
            ),
        })
    }

    /// Sends the model-import request.
    async fn send_request<R: ProcessRunner>(
        &self,
        runner: &mut R,
        max_bytes: usize,
    ) -> Result<(), SidecarFailure> {
        let request = SidecarRequest {
            id: MODEL_REQUEST_ID,
            action: RequestAction::ImportModel,
        };
        info!(id = MODEL_REQUEST_ID, "sidecar: sending model import request");
        self.send(runner, &ClientMessage::Request(request), max_bytes)
            .await
    }

    /// Validates a response message and extracts the [`SidecarModel`].
    async fn expect_model<R: ProcessRunner>(
        &self,
        runner: &mut R,
        message: ServerMessage,
    ) -> Result<SidecarModel, SidecarFailure> {
        let response = match message {
            ServerMessage::Response(response) => *response,
            ServerMessage::Hello(_) => {
                return Err(self.degraded("response", SidecarFailure::MalformedFrame {
                    detail: "expected-response".to_string(),
                }));
            }
        };

        if response.id != MODEL_REQUEST_ID {
            return Err(self.degraded("response", SidecarFailure::MalformedFrame {
                detail: "id-mismatch".to_string(),
            }));
        }

        match response.outcome {
            ResponseOutcome::Model(model) => {
                let _ = runner.kill().await;
                Ok(model)
            }
            ResponseOutcome::Error(body) => {
                let failure = if body.code == "staleCache" {
                    SidecarFailure::StaleCache
                } else {
                    SidecarFailure::SyncFailure {
                        detail: body.message,
                    }
                };
                Err(self.degraded("response", failure))
            }
        }
    }

    /// Reads one server message within the deadline, optionally racing a cancel token.
    async fn read_message<R: ProcessRunner>(
        &self,
        runner: &mut R,
        timeout_ms: u64,
        max_bytes: usize,
        cancel: Option<oneshot::Receiver<()>>,
    ) -> Result<ServerMessage, SidecarFailure> {
        let deadline = Duration::from_millis(timeout_ms);
        let Some(mut rx) = cancel else {
            return read_one(runner, deadline, timeout_ms, max_bytes).await;
        };

        {
            let read = read_one(runner, deadline, timeout_ms, max_bytes);
            tokio::pin!(read);
            tokio::select! {
                biased;
                cancel_res = &mut rx => match cancel_res {
                    Ok(()) => {}
                    Err(_) => return (&mut read).await,
                },
                out = &mut read => return out,
            }
        }

        self.cancel_inflight(runner, max_bytes).await;
        Err(SidecarFailure::Canceled)
    }

    /// Serializes and writes one client message as a framed line.
    async fn send<R: ProcessRunner>(
        &self,
        runner: &mut R,
        message: &ClientMessage,
        max_bytes: usize,
    ) -> Result<(), SidecarFailure> {
        let line = to_line(message, max_bytes).map_err(|e| SidecarFailure::MalformedFrame {
            detail: e.detail().to_string(),
        })?;
        runner
            .write_line(&line)
            .await
            .map_err(|e| SidecarFailure::SyncFailure {
                detail: e.to_string(),
            })
    }

    /// Best-effort: notifies the sidecar of cancellation, then returns [`SidecarFailure::Canceled`].
    async fn cancel_inflight<R: ProcessRunner>(&self, runner: &mut R, max_bytes: usize) {
        let cancel = ClientMessage::Cancel(CancelRequest {
            id: MODEL_REQUEST_ID,
        });
        if let Ok(line) = to_line(&cancel, max_bytes) {
            let _ = runner.write_line(&line).await;
        }
        let _ = runner.kill().await;
    }

    /// Logs a degradation and returns the failure unchanged (a small `?`-friendly helper).
    fn degraded(&self, stage: &str, failure: SidecarFailure) -> SidecarFailure {
        self.log_degradation(stage, &failure);
        failure
    }

    /// Emits a structured warn-level log recording the degradation to the static tier.
    fn log_degradation(&self, stage: &str, failure: &SidecarFailure) {
        warn!(
            stage,
            key = %failure.message_key(),
            degraded_to_static = failure.degraded_to_static(),
            "sidecar degraded to static tier: {failure:?}"
        );
    }
}

/// Reads exactly one server message within `deadline`, mapping every error to a failure.
///
/// A timeout, a transport error, an early end-of-stream, and a malformed frame each become
/// the matching [`SidecarFailure`]; nothing here panics.
async fn read_one<R: ProcessRunner>(
    runner: &mut R,
    deadline: Duration,
    timeout_ms: u64,
    max_bytes: usize,
) -> Result<ServerMessage, SidecarFailure> {
    match tokio::time::timeout(deadline, runner.read_line()).await {
        Err(_) => Err(SidecarFailure::Timeout {
            elapsed_ms: timeout_ms,
        }),
        Ok(Err(transport)) => Err(SidecarFailure::SyncFailure {
            detail: transport.to_string(),
        }),
        Ok(Ok(None)) => Err(SidecarFailure::SyncFailure {
            detail: "sidecar exited before replying".to_string(),
        }),
        Ok(Ok(Some(line))) => {
            decode_line::<ServerMessage>(&line, max_bytes).map_err(|frame| {
                SidecarFailure::MalformedFrame {
                    detail: frame.detail().to_string(),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GradleAnalyzerConfig;
    use crate::gradle::sidecar::model::{ExtensionInfo, SidecarModel};
    use crate::gradle::sidecar::protocol::{ServerHello, SidecarErrorBody, SidecarResponse};
    use crate::gradle::sidecar::runner::FakeRunner;

    /// A client whose request timeout is `timeout_ms` (everything else default).
    fn client_with_timeout(timeout_ms: u64) -> SidecarClient {
        let mut config = GradleAnalyzerConfig::default();
        config.sidecar.request_timeout_ms = timeout_ms;
        SidecarClient::new(ConfigManager::new(config))
    }

    /// A model carrying a `dotnet {}` extension, mirroring the advanced-tier payload.
    fn dotnet_model() -> SidecarModel {
        SidecarModel {
            gradle_version: "8.10".to_string(),
            extensions: vec![ExtensionInfo {
                name: "dotnet".to_string(),
                type_fqn: "com.example.gradle.DotnetExtension".to_string(),
            }],
            ..SidecarModel::default()
        }
    }

    fn server_hello() -> ServerHello {
        ServerHello {
            chosen_version: 1,
            capabilities: vec![Capability::ModelImport, Capability::Cancellation],
        }
    }

    #[tokio::test]
    async fn handshake_then_happy_round_trip_parses_model_with_dotnet_extension() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder()
            .hello(server_hello())
            .response(SidecarResponse {
                id: MODEL_REQUEST_ID,
                outcome: ResponseOutcome::Model(dotnet_model()),
            })
            .build();

        let model = client.import_model(&mut runner).await.expect("model imported");

        let dotnet = model
            .extensions
            .iter()
            .find(|e| e.name == "dotnet")
            .expect("dotnet extension present");
        assert_eq!(dotnet.type_fqn, "com.example.gradle.DotnetExtension");

        // The wire shows the handshake hello followed by the model request.
        assert_eq!(runner.written().len(), 2);
        assert!(runner.written()[0].contains("\"type\":\"hello\""));
        assert!(runner.written()[1].contains("\"type\":\"request\""));
        assert!(runner.was_spawned());
    }

    #[tokio::test]
    async fn version_mismatch_hello_yields_schema_mismatch_degraded_to_static() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder()
            .hello(ServerHello {
                chosen_version: 2,
                capabilities: vec![],
            })
            .build();

        let failure = client.import_model(&mut runner).await.unwrap_err();
        assert_eq!(failure, SidecarFailure::SchemaMismatch { version: 2 });
        assert!(failure.degraded_to_static());
    }

    #[tokio::test(start_paused = true)]
    async fn never_replying_runner_times_out_within_the_config_deadline() {
        let client = client_with_timeout(50);
        let mut runner = FakeRunner::builder().hello(server_hello()).hang().build();

        let failure = client.import_model(&mut runner).await.unwrap_err();
        assert_eq!(failure, SidecarFailure::Timeout { elapsed_ms: 50 });
        assert!(failure.degraded_to_static());
    }

    #[tokio::test]
    async fn canceled_request_resolves_to_canceled_without_hanging() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder().hello(server_hello()).hang().build();

        let (tx, rx) = oneshot::channel();
        tx.send(()).unwrap();

        let failure = client
            .import_model_cancelable(&mut runner, rx)
            .await
            .unwrap_err();
        assert_eq!(failure, SidecarFailure::Canceled);
        assert!(failure.degraded_to_static());
    }

    #[tokio::test]
    async fn sidecar_error_response_maps_to_sync_failure() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder()
            .hello(server_hello())
            .response(SidecarResponse {
                id: MODEL_REQUEST_ID,
                outcome: ResponseOutcome::Error(SidecarErrorBody {
                    code: "syncFailure".to_string(),
                    message: "compileJava failed".to_string(),
                }),
            })
            .build();

        let failure = client.import_model(&mut runner).await.unwrap_err();
        assert_eq!(
            failure,
            SidecarFailure::SyncFailure {
                detail: "compileJava failed".to_string()
            }
        );
    }

    #[tokio::test]
    async fn stale_cache_error_code_maps_to_stale_cache_failure() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder()
            .hello(server_hello())
            .response(SidecarResponse {
                id: MODEL_REQUEST_ID,
                outcome: ResponseOutcome::Error(SidecarErrorBody {
                    code: "staleCache".to_string(),
                    message: "model is stale".to_string(),
                }),
            })
            .build();

        assert_eq!(
            client.import_model(&mut runner).await.unwrap_err(),
            SidecarFailure::StaleCache
        );
    }

    #[tokio::test]
    async fn non_json_reply_after_hello_is_recovered_as_malformed_frame() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder()
            .hello(server_hello())
            .line("this is not json")
            .build();

        let failure = client.import_model(&mut runner).await.unwrap_err();
        assert!(matches!(failure, SidecarFailure::MalformedFrame { .. }));
        assert!(failure.degraded_to_static());
    }

    #[tokio::test]
    async fn early_exit_after_hello_degrades_without_panicking() {
        let client = client_with_timeout(30_000);
        let mut runner = FakeRunner::builder().hello(server_hello()).eof().build();

        let failure = client.import_model(&mut runner).await.unwrap_err();
        assert!(matches!(failure, SidecarFailure::SyncFailure { .. }));
    }
}
