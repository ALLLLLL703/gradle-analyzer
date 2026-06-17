use tower_lsp::lsp_types::{DidOpenTextDocumentParams, MessageType};

use crate::{
    document::DocumentSnapshot,
    services::Backend,
};

impl Backend {
    pub async fn handle_open(&self, params: DidOpenTextDocumentParams) {
        let workspace_root = self
            .runtime
            .workspace
            .find_workspace_root(std::path::Path::new(params.text_document.uri.path()));

        let snapshot = DocumentSnapshot {
            uri: params.text_document.uri.clone(),
            version: params.text_document.version,
            text: params.text_document.text,
            kind: self.runtime.workspace.classify_file(&params.text_document.uri),
            workspace_root,
        };

        self.runtime
            .documents
            .open(&params.text_document.uri, snapshot)
            .await;
        self.client
            .log_message(MessageType::INFO, self.runtime.lang.document_opened())
            .await;
    }
}
