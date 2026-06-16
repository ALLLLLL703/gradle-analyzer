use tower_lsp::lsp_types::Url;

use crate::services::Backend;

impl Backend {
    pub async fn handle_close(&self, uri: Url) {
        self.runtime.documents.close(&uri).await;
        self.client.publish_diagnostics(uri, vec![], None).await;
        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                self.runtime.lang.document_closed(),
            )
            .await;
    }
}
