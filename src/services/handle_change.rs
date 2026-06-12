use tower_lsp::lsp_types::DidChangeTextDocumentParams;

use crate::{document::DocumentSnapShot, services::Backend};

impl Backend {
    pub async fn handle_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let Some(change) = params.content_changes.into_iter().next() else {
            return;
        };

        let snapshot = DocumentSnapShot {
            version,
            text: change.text,
        };

        {
            let mut docs = self.documents.write().await;
            docs.update(&uri, snapshot);
        }
    }
}
