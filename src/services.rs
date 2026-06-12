pub mod diagnostics;
pub mod handle_change;
pub mod handle_close;
pub mod handle_open;

use crate::{
    document::{DocumentSnapShot, DocumentStore},
    services::diagnostics::DiagnosticsManager,
};
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::sync::RwLock;
use tower_lsp::{
    Client, LanguageServer, LspService, Server,
    lsp_types::{
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        InitializeParams, InitializeResult, ServerCapabilities, TextDocumentSyncKind,
    },
};
pub struct Backend {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
    diagnostics: Arc<RwLock<DiagnosticsManager>>,
}

#[tower_lsp::async_trait]
impl<'a> LanguageServer for Backend {
    async fn initialize(
        &self,
        para: InitializeParams,
    ) -> tower_lsp::jsonrpc::Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(tower_lsp::lsp_types::TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }
    async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, para: DidOpenTextDocumentParams) {
        self.publish_placeholder_diagnostic(&para.text_document.uri, &para.text_document.text)
            .await;
        self.handle_open(para).await;
    }
    async fn did_change(&self, para: DidChangeTextDocumentParams) {
        self.publish_placeholder_diagnostic(
            &para.text_document.uri,
            &para
                .clone()
                .content_changes
                .into_iter()
                .next()
                .unwrap()
                .text,
        )
        .await;

        self.handle_change(para).await;
    }

    async fn did_close(&self, para: DidCloseTextDocumentParams) {
        self.handle_close(para.text_document.uri).await;
    }
}

impl Backend {
    pub fn new(client: Client) -> Self {
        let diagnostics = Arc::new(RwLock::new(DiagnosticsManager::default()));
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore {
                documents: HashMap::new(),
            })),
            diagnostics,
        }
    }
}
