//! The [`GradleLanguageServer`]: the `tower-lsp` protocol surface.
//!
//! This type owns the protocol callbacks plus the shared platform services
//! ([`ConfigManager`], [`Translator`]) and the shared [`DocumentLifecycle`]. It stays
//! deliberately THIN: lifecycle notifications delegate to the lifecycle handle; request
//! handlers delegate to the dispatch/deadline helpers and return EMPTY results (feature
//! bodies are Tasks 9-13). The runtime guarantees proven here are capability negotiation,
//! the document lifecycle, generation-gated cancellation, and the bounded-timeout SLA —
//! not feature output.

use tower_lsp::lsp_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams, InitializeParams,
    InitializeResult, InitializedParams, Location, MessageType, ReferenceParams, ServerInfo,
};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result};

use crate::config::ConfigManager;
use crate::i18n::{MessageKey, Translator};
use crate::lsp::capabilities::server_capabilities;
use crate::lsp::deadline::with_deadline;
use crate::lsp::dispatch::run_if_current;
use crate::lsp::lifecycle::DocumentLifecycle;

/// The Gradle language server backend.
///
/// Holds the editor [`Client`], the shared config and translator, and the shared
/// [`DocumentLifecycle`] every feature reads documents from. Constructed once per
/// `LspService`; the lifecycle handle is cheaply cloneable for spawned work.
pub struct GradleLanguageServer {
    client: Client,
    config: ConfigManager,
    translator: Translator,
    lifecycle: DocumentLifecycle,
}

impl GradleLanguageServer {
    /// Creates a backend bound to `client` with the given shared services.
    pub fn new(client: Client, config: ConfigManager, translator: Translator) -> Self {
        Self {
            client,
            config,
            translator,
            lifecycle: DocumentLifecycle::new(),
        }
    }

    /// Returns the shared document lifecycle handle (used by tests and spawned work).
    pub fn lifecycle(&self) -> &DocumentLifecycle {
        &self.lifecycle
    }

    /// Returns the server name and version for the `initialize` response.
    fn server_info() -> ServerInfo {
        ServerInfo {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for GradleLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        let max_message_bytes = self.config.snapshot().transport.max_message_bytes;
        tracing::info!(
            max_message_bytes,
            "handling initialize; advertising full v1 capabilities"
        );
        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(Self::server_info()),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        let message = self.translator.text(MessageKey::ServerInitialized);
        tracing::info!("server initialized");
        self.client.log_message(MessageType::INFO, message).await;
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("handling shutdown");
        let message = self.translator.text(MessageKey::ServerShutdown);
        self.client.log_message(MessageType::INFO, message).await;
        Ok(())
    }

    // --- Document lifecycle (full-text sync into the shared store) ---

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.lifecycle
            .open(doc.uri, doc.version, doc.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        // Full-text sync (FULL): the last content change carries the whole document.
        let Some(change) = params.content_changes.into_iter().next_back() else {
            tracing::warn!(uri = %uri, "did_change carried no content changes; ignoring");
            return;
        };
        self.lifecycle.change(&uri, version, change.text).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.lifecycle.close(&params.text_document.uri).await;
    }

    // --- Static-tier request seams (read snapshots; NEVER wait on the model) ---

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        // Static tier: answered from the in-memory snapshot, bypassing `with_deadline`.
        // Feature body is Task 10; here it proves the snapshot read + dispatch gate.
        let uri = params.text_document.uri;
        let Some(generation) = self.lifecycle.current_generation(&uri).await else {
            return Ok(None);
        };
        let token = self.lifecycle.token_for(uri.clone(), generation);
        let symbols = run_if_current(&self.lifecycle, &token, async {
            // Task 10 computes real symbols from the snapshot; the seam returns empty.
            let _snapshot = self.lifecycle.snapshot(&uri).await;
            Vec::new()
        })
        .await;
        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    // --- Model-dependent request seam (bounded by the config deadline) ---

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        // Model tier: the future may consult the sidecar (Task 11). It MUST be bounded so
        // it never stalls the loop; on deadline we return an empty/pending result.
        let deadline_ms = self.config.snapshot().sidecar.model_request_deadline_ms;
        let uri = params.text_document_position.text_document.uri;
        let model_work = async {
            // Task 11 fills this with sidecar-backed items; the seam yields nothing.
            let _snapshot = self.lifecycle.snapshot(&uri).await;
            Vec::new()
        };
        let items = match with_deadline(model_work, deadline_ms).await.into_option() {
            Some(items) => items,
            None => {
                tracing::warn!(deadline_ms, "completion model deadline exceeded; empty result");
                Vec::new()
            }
        };
        Ok(Some(CompletionResponse::Array(items)))
    }

    // --- Remaining v1 seams: advertised, empty until their task lands ---

    async fn goto_definition(
        &self,
        _params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        // Feature body: Task 12.
        Ok(None)
    }

    async fn references(&self, _params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        // Feature body: Task 12.
        Ok(None)
    }

    async fn hover(&self, _params: HoverParams) -> Result<Option<Hover>> {
        // Feature body: later task.
        Ok(None)
    }

    async fn code_action(
        &self,
        _params: tower_lsp::lsp_types::CodeActionParams,
    ) -> Result<Option<tower_lsp::lsp_types::CodeActionResponse>> {
        // Feature body: Task 13.
        Ok(None)
    }
}
