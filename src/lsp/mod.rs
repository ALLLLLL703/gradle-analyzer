//! The LSP protocol layer.
//!
//! [`GradleLanguageServer`] implements `tower_lsp::LanguageServer` and owns the
//! protocol callbacks; [`capabilities`] centralizes what the server advertises. Feature
//! logic lives in the `gradle` modules and is wired in by later tasks.

pub mod capabilities;
pub mod server;

pub use server::GradleLanguageServer;
