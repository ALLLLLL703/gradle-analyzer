use tower_lsp::lsp_types::DidChangeTextDocumentParams;

use crate::{document::DocumentSnapshot, services::Backend};

impl Backend {
    pub async fn handle_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let Some(change) = params.content_changes.into_iter().next() else {
            return;
        };

        let snapshot = DocumentSnapshot {
            uri: uri.clone(),
            version,
            text: change.text,
        };

        self.runtime.documents.update(&uri, snapshot).await;
        self.client
            .log_message(tower_lsp::lsp_types::MessageType::INFO, self.runtime.lang.document_changed())
            .await;
    }
}
