//! JVM Tooling-API sidecar contract and process orchestration.
//!
//! This module defines the **Rust side** of the sidecar IPC contract and proves it end to
//! end with a fake in-process runner — no Java and no real child process (that is Task 14).
//! The real sidecar will be a Gradle Tooling-API `BuildAction` launched via the project
//! wrapper, exchanging line-delimited JSON over a child process's stdio.
//!
//! Layers, smallest to largest:
//!
//! - [`framing`]: one `\n`-terminated JSON object per line, with a max-message-size guard
//!   (`transport.max_message_bytes`) and recoverable [`framing::FrameError`]s.
//! - [`protocol`]: the [`protocol::PROTOCOL_VERSION`] constant, the handshake
//!   ([`protocol::ClientHello`]/[`protocol::ServerHello`]) with version + capability
//!   negotiation, and the request/response/cancel envelopes.
//! - [`model`]: the [`model::SidecarModel`] payload the real `BuildAction` will emit
//!   (designed now, populated in Task 14).
//! - [`runner`]: the [`runner::ProcessRunner`] transport seam plus the scripted
//!   [`runner::FakeRunner`]. The production wrapper-launching runner lands in Task 14
//!   behind this same trait.
//! - [`failure`]: the [`failure::SidecarFailure`] taxonomy, each variant mapped to a
//!   localized [`crate::i18n::MessageKey`] status and a `degraded_to_static` signal.
//! - [`client`]: the [`client::SidecarClient`] that runs the handshake + a request/response
//!   round-trip over any [`runner::ProcessRunner`], enforcing the config-backed timeout and
//!   always degrading to the static tier — never panicking.
//!
//! Task 14 adds the REAL runner on top of this same contract:
//!
//! - [`launch`]: pure [`launch::plan_launch`] that validates the JVM / wrapper / installation
//!   / compiled classes and builds the `java` argv (each gap → a typed [`failure::SidecarFailure`]).
//! - [`wrapper_runner`]: the production [`wrapper_runner::WrapperRunner`], a tokio child
//!   process speaking the framed protocol over stdio (the Gradle daemon's stderr is inherited
//!   so it never pollutes the stdout protocol channel).
//! - [`cache`]: a [`cache::ModelCache`] keyed by Gradle version + classpath fingerprint, so a
//!   prior import is reused while fresh and a changed key degrades to
//!   [`failure::SidecarFailure::StaleCache`].
//! - [`service`]: the [`service::SidecarService`] that acquires a `max_concurrent` permit,
//!   plans + launches the sidecar, drives the client (timeout + cancel), and caches the model.
//!
//! # Example
//!
//! ```
//! # async fn run() {
//! use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
//! use gradle_analyzer::gradle::sidecar::{
//!     SidecarClient, FakeRunner,
//!     protocol::{Capability, ServerHello, SidecarResponse, ResponseOutcome},
//!     model::SidecarModel,
//! };
//!
//! let client = SidecarClient::new(ConfigManager::new(GradleAnalyzerConfig::default()));
//! let mut runner = FakeRunner::builder()
//!     .hello(ServerHello { chosen_version: 1, capabilities: vec![Capability::ModelImport] })
//!     .response(SidecarResponse { id: 1, outcome: ResponseOutcome::Model(SidecarModel::default()) })
//!     .build();
//!
//! match client.import_model(&mut runner).await {
//!     Ok(model) => println!("imported gradle {}", model.gradle_version),
//!     Err(failure) => assert!(failure.degraded_to_static()),
//! }
//! # }
//! ```

pub mod cache;
pub mod client;
pub mod failure;
pub mod framing;
pub mod launch;
pub mod model;
pub mod protocol;
pub mod runner;
pub mod service;
pub mod wrapper_runner;

pub use client::SidecarClient;
pub use failure::SidecarFailure;
pub use model::SidecarModel;
pub use runner::{FakeRunner, ProcessRunner, RunnerError};
pub use service::SidecarService;
pub use wrapper_runner::{CommandSpec, WrapperRunner};
