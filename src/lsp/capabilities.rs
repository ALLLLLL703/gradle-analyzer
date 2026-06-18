//! Builds the server's advertised [`ServerCapabilities`].
//!
//! Capability advertisement lives in ONE auditable place rather than scattered across
//! handlers. Task 8 advertises the full v1 set the protocol needs — text sync FULL plus
//! documentSymbol, completion (with trigger characters), definition, references,
//! codeAction, and hover — so editors negotiate the complete surface up front. The
//! handlers behind these capabilities are thin seams that return empty results until
//! their owning task (Tasks 9-13) fills them in.

use tower_lsp::lsp_types::{
    CompletionOptions, HoverProviderCapability, OneOf, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind,
};

/// Completion trigger characters for Gradle build scripts.
///
/// These open the contexts later completion work keys off: `{` (block bodies), `.`
/// (member / `libs.` accessors), `'` and `"` (dependency-coordinate strings), and `(`
/// (call arguments). Advertised now so the client sends completion requests at the right
/// moments before the feature body exists.
pub const COMPLETION_TRIGGER_CHARACTERS: [&str; 5] = ["{", ".", "'", "\"", "("];

/// Returns the baseline capabilities advertised during `initialize`.
///
/// An alias of [`server_capabilities`] kept for the Task-1 call site and its doctest;
/// the full v1 set is built in one place.
pub fn baseline_capabilities() -> ServerCapabilities {
    server_capabilities()
}

/// Returns the full v1 [`ServerCapabilities`] the server advertises.
///
/// # Example
///
/// ```
/// use gradle_analyzer::lsp::capabilities::server_capabilities;
/// use tower_lsp::lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};
///
/// let caps = server_capabilities();
/// // Full-text document sync.
/// assert!(matches!(
///     caps.text_document_sync,
///     Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
/// ));
/// // The full v1 feature surface is advertised (handlers fill in by later tasks).
/// assert!(caps.document_symbol_provider.is_some());
/// assert!(caps.completion_provider.is_some());
/// assert!(caps.definition_provider.is_some());
/// assert!(caps.references_provider.is_some());
/// assert!(caps.code_action_provider.is_some());
/// assert!(caps.hover_provider.is_some());
///
/// // Completion advertises the build-script trigger characters.
/// let triggers = caps.completion_provider.unwrap().trigger_characters.unwrap();
/// assert!(triggers.iter().any(|c| c == "."));
/// assert!(triggers.iter().any(|c| c == "{"));
/// ```
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(
                COMPLETION_TRIGGER_CHARACTERS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            ..CompletionOptions::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(tower_lsp::lsp_types::CodeActionProviderCapability::Simple(
            true,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    }
}
