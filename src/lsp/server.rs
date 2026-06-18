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
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    InitializeParams, InitializeResult, InitializedParams, InsertTextFormat, Location,
    MessageType, Position, Range, ReferenceParams, ServerInfo, Url,
};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result};

use crate::config::ConfigManager;
use crate::gradle::completion::{self, Candidate, CandidateKind, CompletionServices};
use crate::gradle::diagnostics::{Diagnostic, Severity, compute_diagnostics};
use crate::gradle::navigation::lsp::{NavQuery, navigate};
use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{SemanticInput, analyze_documents};
use crate::gradle::workspace::{DslLanguage, TrackedDocument};
use crate::i18n::{MessageKey, Translator};
use crate::lsp::capabilities::server_capabilities;
use crate::lsp::deadline::with_deadline;
use crate::lsp::dispatch::run_if_current;
use crate::lsp::lifecycle::DocumentLifecycle;
use crate::util::line_index::LineIndex;

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

    /// Computes and publishes static diagnostics for `uri` (the static-tier seam).
    ///
    /// Reads the in-memory snapshot, gates on the per-DSL feature toggle, parses + analyzes
    /// just this document, runs [`compute_diagnostics`], converts the LSP-free model to
    /// protocol diagnostics at this boundary, and publishes them. A clean (or
    /// feature-disabled, or unrecognized) document publishes an EMPTY array, which clears any
    /// previously shown diagnostics — the clear-on-fix path. Never waits on the model tier.
    async fn publish_diagnostics(&self, uri: &Url) {
        let Some(doc) = self.lifecycle.snapshot(uri).await else {
            return;
        };
        let diagnostics = self.diagnostics_for(&doc);
        let line_index = LineIndex::new(doc.text_arc());
        let lsp_diagnostics = diagnostics
            .iter()
            .map(|diag| self.to_lsp_diagnostic(diag, &line_index))
            .collect();
        self.client
            .publish_diagnostics(uri.clone(), lsp_diagnostics, Some(doc.version()))
            .await;
    }

    /// Runs the static analysis pipeline for `doc`, honoring the per-DSL feature toggle.
    ///
    /// Returns an empty vector when the document's DSL is disabled or it has no DSL (a
    /// version catalog / unknown file), so the published result is an explicit clear.
    fn diagnostics_for(&self, doc: &TrackedDocument) -> Vec<Diagnostic> {
        let features = &self.config.snapshot().features;
        let enabled = match doc.kind().dsl() {
            Some(DslLanguage::Kotlin) => features.enable_kotlin_dsl,
            Some(DslLanguage::Groovy) => features.enable_groovy_dsl,
            None => return Vec::new(),
        };
        if !enabled {
            return Vec::new();
        }
        let text = doc.text();
        let parse = match doc.kind().dsl() {
            Some(DslLanguage::Kotlin) => parse_kotlin(text),
            _ => parse_groovy(text),
        };
        let input = SemanticInput::from_tracked(&workspace_root(doc), doc);
        let graph = analyze_documents(std::slice::from_ref(&input));
        match graph.document(&input.id) {
            Some(semantics) => compute_diagnostics(doc, &parse, semantics),
            None => Vec::new(),
        }
    }

    /// Converts one LSP-free [`Diagnostic`] to a protocol diagnostic at the server boundary.
    fn to_lsp_diagnostic(&self, diag: &Diagnostic, line_index: &LineIndex) -> LspDiagnostic {
        let args: Vec<&str> = diag.args.iter().map(String::as_str).collect();
        let message = self.translator.get_text(diag.message_key, &args);
        LspDiagnostic {
            range: to_range(diag, line_index),
            severity: Some(to_severity(diag.severity)),
            source: Some(env!("CARGO_PKG_NAME").to_string()),
            message,
            ..LspDiagnostic::default()
        }
    }

    /// Runs the static completion engine for `doc` at `position` and converts to LSP items.
    ///
    /// Parses the document for its DSL, builds the semantic graph from the document plus any
    /// on-disk version catalog (so `libs.*` accessors resolve), maps the LSP position to a
    /// byte offset, and converts each engine [`Candidate`] to a `CompletionItem` at this
    /// boundary. Never waits on the sidecar.
    fn completion_items(
        &self,
        doc: &TrackedDocument,
        position: Position,
        max_candidates: usize,
    ) -> Vec<CompletionItem> {
        let text = doc.text();
        let parse = match doc.kind().dsl() {
            Some(DslLanguage::Kotlin) => parse_kotlin(text),
            _ => parse_groovy(text),
        };
        let inputs = completion::workspace_inputs(doc);
        let graph = analyze_documents(&inputs);
        let offset = completion::byte_offset_at(text, position.line, position.character);
        let services = CompletionServices::new(&self.translator, max_candidates);
        completion::complete(doc, &parse, &graph, offset, &services)
            .into_iter()
            .map(to_completion_item)
            .collect()
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
        let uri = doc.uri.clone();
        self.lifecycle.open(doc.uri, doc.version, doc.text).await;
        self.publish_diagnostics(&uri).await;
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
        self.publish_diagnostics(&uri).await;
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
        // Outline is computed from syntax + semantics (Task 10) behind the dispatch gate.
        let uri = params.text_document.uri;
        let Some(generation) = self.lifecycle.current_generation(&uri).await else {
            return Ok(None);
        };
        let token = self.lifecycle.token_for(uri.clone(), generation);
        let symbols = run_if_current(&self.lifecycle, &token, async {
            match self.lifecycle.snapshot(&uri).await {
                Some(doc) => crate::gradle::symbols::outline_lsp(&doc),
                None => Vec::new(),
            }
        })
        .await;
        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    // --- Model-dependent request seam (bounded by the config deadline) ---

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        // Static-first (Task 11): the engine answers from the snapshot + semantic graph, well
        // within the model deadline that still bounds the loop. Computed behind the deadline
        // seam for uniformity; conversion to protocol items happens at this boundary.
        let deadline_ms = self.config.snapshot().sidecar.model_request_deadline_ms;
        let max_candidates = self.config.snapshot().completion.max_candidates;
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let work = async {
            let Some(doc) = self.lifecycle.snapshot(&uri).await else {
                return Vec::new();
            };
            self.completion_items(&doc, position, max_candidates)
        };
        let items = match with_deadline(work, deadline_ms).await.into_option() {
            Some(items) => items,
            None => {
                tracing::warn!(deadline_ms, "completion deadline exceeded; empty result");
                Vec::new()
            }
        };
        Ok(Some(CompletionResponse::Array(items)))
    }

    // --- Remaining v1 seams: advertised, empty until their task lands ---

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        // Static tier (Task 12): resolve from the snapshot's semantic graph behind the
        // dispatch gate; conversion to protocol Locations happens at the navigation boundary.
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some(generation) = self.lifecycle.current_generation(&uri).await else {
            return Ok(None);
        };
        let token = self.lifecycle.token_for(uri.clone(), generation);
        let locations = run_if_current(&self.lifecycle, &token, async {
            match self.lifecycle.snapshot(&uri).await {
                Some(doc) => navigate(&doc, position, NavQuery::Definition),
                None => Vec::new(),
            }
        })
        .await
        .unwrap_or_default();
        if locations.is_empty() {
            return Ok(None);
        }
        Ok(Some(GotoDefinitionResponse::Array(locations)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        // Static tier (Task 12): every reference site for the symbol under the cursor in the
        // current document. Empty results return `None`; the dispatch gate drops stale work.
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let Some(generation) = self.lifecycle.current_generation(&uri).await else {
            return Ok(None);
        };
        let token = self.lifecycle.token_for(uri.clone(), generation);
        let locations = run_if_current(&self.lifecycle, &token, async {
            match self.lifecycle.snapshot(&uri).await {
                Some(doc) => navigate(&doc, position, NavQuery::References),
                None => Vec::new(),
            }
        })
        .await
        .unwrap_or_default();
        if locations.is_empty() {
            return Ok(None);
        }
        Ok(Some(locations))
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

/// Maps an LSP-free diagnostic span to a protocol [`Range`] via the document line index.
fn to_range(diag: &Diagnostic, line_index: &LineIndex) -> Range {
    let start = line_index.line_col(diag.span.start);
    let end = line_index.line_col(diag.span.end());
    Range {
        start: Position { line: start.line, character: start.character },
        end: Position { line: end.line, character: end.character },
    }
}

/// Maps the internal [`Severity`] to the protocol [`DiagnosticSeverity`].
fn to_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Information => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

/// Converts an engine [`Candidate`] to a protocol [`CompletionItem`] at the boundary.
///
/// The label is the source identifier; `detail` is the already-localized text; an
/// `insert_text` override (scaffolds, repository calls) is emitted as an LSP snippet so
/// placeholders work, otherwise the label is inserted as plain text.
fn to_completion_item(candidate: Candidate) -> CompletionItem {
    let kind = to_item_kind(candidate.kind);
    let (insert_text, format) = match candidate.insert_text {
        Some(snippet) => (Some(snippet), Some(InsertTextFormat::SNIPPET)),
        None => (None, None),
    };
    CompletionItem {
        label: candidate.label,
        kind: Some(kind),
        detail: Some(candidate.detail),
        insert_text,
        insert_text_format: format,
        ..CompletionItem::default()
    }
}

/// Maps an engine [`CandidateKind`] to the closest protocol [`CompletionItemKind`].
fn to_item_kind(kind: CandidateKind) -> CompletionItemKind {
    match kind {
        CandidateKind::BlockKeyword => CompletionItemKind::KEYWORD,
        CandidateKind::DependencyConfiguration => CompletionItemKind::FUNCTION,
        CandidateKind::CatalogAccessor => CompletionItemKind::CONSTANT,
        CandidateKind::CoordinateScaffold => CompletionItemKind::SNIPPET,
        CandidateKind::PluginId => CompletionItemKind::MODULE,
        CandidateKind::Repository => CompletionItemKind::FUNCTION,
        CandidateKind::TaskName => CompletionItemKind::VALUE,
        CandidateKind::ProjectPath => CompletionItemKind::MODULE,
        CandidateKind::ImportHint => CompletionItemKind::REFERENCE,
        CandidateKind::PluginContributed => CompletionItemKind::FIELD,
    }
}

/// Resolves the workspace root for `doc`, falling back to its parent directory.
///
/// Single-document analysis only needs a root to derive the document's relative id; the
/// detected root (or the file's own directory) is sufficient and never panics.
fn workspace_root(doc: &TrackedDocument) -> std::path::PathBuf {
    use crate::gradle::workspace::detect_workspace_root;
    doc.uri()
        .to_file_path()
        .ok()
        .and_then(|path| {
            detect_workspace_root(&path)
                .map(|root| root.path().to_path_buf())
                .or_else(|| path.parent().map(std::path::Path::to_path_buf))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
