use tower_lsp::lsp_types::{DidOpenTextDocumentParams, MessageType};

use crate::{document::DocumentSnapshot, services::Backend};

impl Backend {
    pub async fn handle_open(&self, params: DidOpenTextDocumentParams) {
        let snapshot = DocumentSnapshot {
            uri: params.text_document.uri.clone(),
            version: params.text_document.version,
            text: params.text_document.text,
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
