//! The LSP protocol layer.
//!
//! [`GradleLanguageServer`] implements `tower_lsp::LanguageServer` and owns the protocol
//! callbacks; the runtime is split into focused modules so the server stays thin:
//!
//! - [`capabilities`] — the single place the advertised [`ServerCapabilities`] are built.
//! - [`lifecycle`] — the shared [`lifecycle::DocumentLifecycle`]: the open/change/close
//!   document store plus per-URI generations for cancellation.
//! - [`dispatch`] — [`dispatch::run_if_current`]: deliver a result only if its generation
//!   is still current, else discard it (the supersede / cancellation seam).
//! - [`deadline`] — [`deadline::with_deadline`]: bound a model-dependent future so it can
//!   never stall the event loop.
//!
//! [`ServerCapabilities`]: tower_lsp::lsp_types::ServerCapabilities

pub mod capabilities;
pub mod deadline;
pub mod dispatch;
pub mod lifecycle;
pub mod server;

pub use server::GradleLanguageServer;
