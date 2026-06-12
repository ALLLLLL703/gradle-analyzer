use tower_lsp::lsp_types::{DidOpenTextDocumentParams, MessageType};

use crate::{document::DocumentSnapShot, services::Backend};

impl Backend {
    pub async fn handle_open(&self, params: DidOpenTextDocumentParams) {
        let snapshot = DocumentSnapShot {
            version: params.text_document.version,
            text: params.text_document.text,
        };

        {
            let mut docs = self.documents.write().await;
            docs.open(&params.text_document.uri, snapshot);
        }
        self.client
            .log_message(MessageType::INFO, "document opened")
            .await;
    }
}
