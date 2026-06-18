//! The [`Application`]: composition root and server loop owner.
//!
//! [`Application::run`] wires real stdin/stdout; [`Application::run_with_io`] runs the
//! same `tower-lsp` loop over any async reader/writer, so an in-memory duplex pipe can
//! drive the protocol in tests. The application builds the shared [`ConfigManager`] and
//! [`Translator`], then hands them to the [`GradleLanguageServer`].

use tokio::io::{AsyncRead, AsyncWrite};
use tower_lsp::{LspService, Server};

use crate::config::{ConfigManager, ConfigSources, GradleAnalyzerConfig};
use crate::i18n::Translator;
use crate::lsp::GradleLanguageServer;
use crate::util::paths;

/// The server composition root.
///
/// Holds the resolved shared services so both the real-stdio and in-memory entry points
/// build the backend identically.
pub struct Application {
    config: ConfigManager,
    translator: Translator,
}

impl Application {
    /// Builds an application from config sources, falling back to defaults on error.
    ///
    /// A config load/validation failure does NOT abort startup: the server logs the
    /// problem and runs on built-in defaults, since a usable static tier must survive a
    /// bad config file.
    pub fn bootstrap() -> Self {
        let sources = resolve_default_sources();
        let config = match ConfigManager::from_sources(sources) {
            Ok(manager) => manager,
            Err(err) => {
                tracing::error!(error = %err, "config load failed; using built-in defaults");
                ConfigManager::new(GradleAnalyzerConfig::default())
            }
        };
        Self {
            config,
            translator: Translator::new(),
        }
    }

    /// Constructs an application from explicit shared services (used by tests).
    pub fn with_services(config: ConfigManager, translator: Translator) -> Self {
        Self { config, translator }
    }

    /// Runs the server over real stdin/stdout. Returns when the client disconnects.
    pub async fn run(self) {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        self.run_with_io(stdin, stdout).await;
    }

    /// Runs the `tower-lsp` server loop over arbitrary async IO.
    ///
    /// This is the testable seam: an in-memory duplex pair drives
    /// `initialize`/`initialized`/`shutdown` without touching real stdio.
    pub async fn run_with_io<R, W>(self, reader: R, writer: W)
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let config = self.config;
        let translator = self.translator;
        let (service, socket) =
            LspService::new(move |client| GradleLanguageServer::new(client, config, translator));
        Server::new(reader, writer, socket).serve(service).await;
    }
}

/// Resolves the default workspace + user config sources for a real run.
fn resolve_default_sources() -> ConfigSources {
    let workspace = std::env::current_dir()
        .ok()
        .map(|cwd| paths::workspace_config_path(&cwd));
    ConfigSources {
        user: paths::user_config_path(),
        workspace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::probe::{encode_frame, notification, read_frame_async, request};
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn duplex_initialize_then_shutdown_returns_valid_results() {
        // client_* is the test's view; server_* is what the server loop reads/writes.
        let (mut client_to_server, server_reader) = tokio::io::duplex(64 * 1024);
        let (server_writer, mut server_to_client) = tokio::io::duplex(64 * 1024);

        let app = Application::with_services(
            ConfigManager::new(GradleAnalyzerConfig::default()),
            Translator::new(),
        );
        let server = tokio::spawn(app.run_with_io(server_reader, server_writer));

        // Drive initialize.
        let init = request(1, "initialize", serde_json::json!({ "capabilities": {} }));
        client_to_server
            .write_all(&encode_frame(&init))
            .await
            .unwrap();
        let init_response = read_frame_async(&mut server_to_client).await.unwrap();

        assert_eq!(init_response["id"], 1);
        let capabilities = &init_response["result"]["capabilities"];
        assert!(
            capabilities.is_object(),
            "initialize result must carry capabilities, got: {init_response}"
        );
        // text sync FULL == TextDocumentSyncKind::FULL == 1
        assert_eq!(capabilities["textDocumentSync"], 1);
        assert_eq!(init_response["result"]["serverInfo"]["name"], "gradle-analyzer");

        // initialized notification (no response expected).
        let initialized = notification("initialized", serde_json::json!({}));
        client_to_server
            .write_all(&encode_frame(&initialized))
            .await
            .unwrap();

        // shutdown request.
        let shutdown = request(2, "shutdown", serde_json::Value::Null);
        client_to_server
            .write_all(&encode_frame(&shutdown))
            .await
            .unwrap();

        // Read frames until the shutdown response (id 2) arrives, skipping any
        // server-initiated log notifications.
        let shutdown_response = loop {
            let frame = read_frame_async(&mut server_to_client).await.unwrap();
            if frame.get("id") == Some(&serde_json::json!(2)) {
                break frame;
            }
        };
        assert!(shutdown_response["result"].is_null());
        assert!(
            shutdown_response.get("error").is_none(),
            "shutdown returned error: {shutdown_response}"
        );

        // Close the client side so the server loop terminates, then join.
        drop(client_to_server);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
    }

    #[tokio::test]
    async fn initialize_advertises_full_v1_capability_set() {
        let (mut client_to_server, server_reader) = tokio::io::duplex(64 * 1024);
        let (server_writer, mut server_to_client) = tokio::io::duplex(64 * 1024);
        let app = Application::with_services(
            ConfigManager::new(GradleAnalyzerConfig::default()),
            Translator::new(),
        );
        let server = tokio::spawn(app.run_with_io(server_reader, server_writer));

        let init = request(1, "initialize", serde_json::json!({ "capabilities": {} }));
        client_to_server.write_all(&encode_frame(&init)).await.unwrap();
        let response = read_until_id(&mut server_to_client, 1).await;

        let caps = &response["result"]["capabilities"];
        assert_eq!(caps["textDocumentSync"], 1, "text sync FULL: {response}");
        assert!(caps.get("documentSymbolProvider").is_some(), "{response}");
        assert!(caps.get("completionProvider").is_some(), "{response}");
        assert!(caps.get("definitionProvider").is_some(), "{response}");
        assert!(caps.get("referencesProvider").is_some(), "{response}");
        assert!(caps.get("codeActionProvider").is_some(), "{response}");
        assert!(caps.get("hoverProvider").is_some(), "{response}");
        let triggers = &caps["completionProvider"]["triggerCharacters"];
        assert!(triggers.as_array().unwrap().iter().any(|c| c == "."));
        assert!(triggers.as_array().unwrap().iter().any(|c| c == "{"));

        drop(client_to_server);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
    }

    #[tokio::test]
    async fn lifecycle_open_change_close_is_observable_via_document_symbol() {
        let (mut client_to_server, server_reader) = tokio::io::duplex(64 * 1024);
        let (server_writer, mut server_to_client) = tokio::io::duplex(64 * 1024);
        let app = Application::with_services(
            ConfigManager::new(GradleAnalyzerConfig::default()),
            Translator::new(),
        );
        let server = tokio::spawn(app.run_with_io(server_reader, server_writer));

        let uri = "file:///proj/build.gradle.kts";
        send(&mut client_to_server, &request(1, "initialize", serde_json::json!({ "capabilities": {} }))).await;
        let _ = read_until_id(&mut server_to_client, 1).await;
        send(&mut client_to_server, &notification("initialized", serde_json::json!({}))).await;

        // Before didOpen: documentSymbol has no open snapshot -> null result.
        send(&mut client_to_server, &document_symbol_request(2, uri)).await;
        let before = read_until_id(&mut server_to_client, 2).await;
        assert!(before["result"].is_null(), "closed doc -> null: {before}");

        // didOpen makes the snapshot present -> documentSymbol returns an (empty) array.
        send(&mut client_to_server, &did_open(uri, 1, "plugins {}")).await;
        send(&mut client_to_server, &document_symbol_request(3, uri)).await;
        let opened = read_until_id(&mut server_to_client, 3).await;
        assert!(opened["result"].is_array(), "open doc -> array: {opened}");

        // didChange replaces the snapshot; still observable (array).
        send(&mut client_to_server, &did_change(uri, 2, "plugins { java }")).await;
        send(&mut client_to_server, &document_symbol_request(4, uri)).await;
        let changed = read_until_id(&mut server_to_client, 4).await;
        assert!(changed["result"].is_array(), "changed doc -> array: {changed}");

        // didClose removes it -> back to null.
        send(&mut client_to_server, &did_close(uri)).await;
        send(&mut client_to_server, &document_symbol_request(5, uri)).await;
        let closed = read_until_id(&mut server_to_client, 5).await;
        assert!(closed["result"].is_null(), "closed doc -> null: {closed}");

        drop(client_to_server);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
    }

    async fn send<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: &serde_json::Value) {
        writer.write_all(&encode_frame(value)).await.unwrap();
    }

    async fn read_until_id<R: AsyncRead + Unpin>(reader: &mut R, id: i64) -> serde_json::Value {
        loop {
            let frame = read_frame_async(reader).await.unwrap();
            if frame.get("id") == Some(&serde_json::json!(id)) {
                return frame;
            }
        }
    }

    fn document_symbol_request(id: i64, uri: &str) -> serde_json::Value {
        request(
            id,
            "textDocument/documentSymbol",
            serde_json::json!({ "textDocument": { "uri": uri } }),
        )
    }

    fn did_open(uri: &str, version: i32, text: &str) -> serde_json::Value {
        notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri, "languageId": "gradle", "version": version, "text": text
                }
            }),
        )
    }

    fn did_change(uri: &str, version: i32, text: &str) -> serde_json::Value {
        notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [ { "text": text } ]
            }),
        )
    }

    fn did_close(uri: &str) -> serde_json::Value {
        notification(
            "textDocument/didClose",
            serde_json::json!({ "textDocument": { "uri": uri } }),
        )
    }
}
