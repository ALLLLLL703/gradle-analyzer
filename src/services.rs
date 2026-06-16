pub mod diagnostics;
pub mod handle_change;
pub mod handle_close;
pub mod handle_open;

use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use tower_lsp::{
    Client, LanguageServer,
    lsp_types::{
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        InitializeParams, InitializeResult, ServerCapabilities, TextDocumentSyncKind, Url,
    },
};

use crate::{
    config::{default::default_runtime_config, manager::ConfigManager},
    document::{DocumentSnapshot, DocumentStore},
    i18n::LangHelper,
};

pub struct Backend {
    client: Client,
    runtime: RuntimeServices,
}

#[derive(Clone)]
pub struct RuntimeServices {
    pub documents: DocumentService,
    pub diagnostics: DiagnosticsService,
    pub config: Arc<ConfigManager>,
    pub lang: Arc<LangHelper>,
}

#[derive(Clone)]
pub struct DocumentService {
    store: Arc<RwLock<DocumentStore>>,
}

#[derive(Clone)]
pub struct DiagnosticsService {
    config: Arc<ConfigManager>,
    lang: Arc<LangHelper>,
}

impl DocumentService {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(DocumentStore {
                documents: HashMap::new(),
            })),
        }
    }

    pub async fn open(&self, uri: &Url, doc: DocumentSnapshot) {
        self.store.write().await.open(uri, doc);
    }

    pub async fn update(&self, uri: &Url, doc: DocumentSnapshot) {
        self.store.write().await.update(uri, doc);
    }

    pub async fn close(&self, uri: &Url) {
        self.store.write().await.close(uri);
    }
}

impl Backend {
    pub fn new(client: Client) -> Self {
        let config = Arc::new(ConfigManager::new(default_runtime_config()));
        let lang = Arc::new(LangHelper::default());

        Self {
            client,
            runtime: RuntimeServices {
                documents: DocumentService::new(),
                diagnostics: DiagnosticsService {
                    config: Arc::clone(&config),
                    lang: Arc::clone(&lang),
                },
                config,
                lang,
            },
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        _para: InitializeParams,
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
        self.runtime
            .diagnostics
            .publish_placeholder_diagnostic(&self.client, &para.text_document.uri, &para.text_document.text)
            .await;
        self.handle_open(para).await;
    }

    async fn did_change(&self, para: DidChangeTextDocumentParams) {
        let changed_text = para
            .content_changes
            .first()
            .map(|change| change.text.clone())
            .unwrap_or_default();

        self.runtime
            .diagnostics
            .publish_placeholder_diagnostic(&self.client, &para.text_document.uri, &changed_text)
            .await;

        self.handle_change(para).await;
    }

    async fn did_close(&self, para: DidCloseTextDocumentParams) {
        self.handle_close(para.text_document.uri).await;
    }
}
