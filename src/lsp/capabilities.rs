//! Builds the server's advertised [`ServerCapabilities`].
//!
//! Task 1 advertises only the bootstrap-safe baseline: full-text document sync. Later
//! tasks extend this builder as they wire real feature handlers, so capability
//! advertisement stays in one auditable place rather than scattered across handlers.

use tower_lsp::lsp_types::{
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
};

/// Returns the baseline capabilities advertised during `initialize`.
///
/// # Example
///
/// ```
/// use gradle_analyzer::lsp::capabilities::baseline_capabilities;
/// use tower_lsp::lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};
///
/// let caps = baseline_capabilities();
/// assert!(matches!(
///     caps.text_document_sync,
///     Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
/// ));
/// ```
pub fn baseline_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        ..ServerCapabilities::default()
    }
}
