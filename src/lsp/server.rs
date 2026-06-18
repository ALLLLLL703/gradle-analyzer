//! The [`GradleLanguageServer`]: the `tower-lsp` protocol surface.
//!
//! This type owns the protocol callbacks and the shared platform services
//! ([`ConfigManager`], [`Translator`]). Task 1 implements only the lifecycle seams
//! (`initialize`, `initialized`, `shutdown`) with real capability advertisement;
//! feature handlers are added by later tasks. Heavy logic must NOT live here — the
//! server delegates to feature modules as they arrive.

use tower_lsp::lsp_types::{
    InitializeParams, InitializeResult, InitializedParams, MessageType, ServerInfo,
};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result};

use crate::config::ConfigManager;
use crate::i18n::{MessageKey, Translator};
use crate::lsp::capabilities::baseline_capabilities;

/// The Gradle language server backend.
///
/// Holds the editor [`Client`] plus the shared config and translator so every handler
/// reads mutable values from [`ConfigManager`] and renders user-facing text through
/// [`Translator`]. Clone-free: constructed once per `LspService`.
pub struct GradleLanguageServer {
    client: Client,
    config: ConfigManager,
    translator: Translator,
}

impl GradleLanguageServer {
    /// Creates a backend bound to `client` with the given shared services.
    pub fn new(client: Client, config: ConfigManager, translator: Translator) -> Self {
        Self {
            client,
            config,
            translator,
        }
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
            "handling initialize; advertising baseline capabilities"
        );
        Ok(InitializeResult {
            capabilities: baseline_capabilities(),
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
}
