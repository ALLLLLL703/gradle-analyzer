use tower_lsp::lsp_types::Url;

use crate::services::Backend;

impl Backend {
    pub async fn handle_close(&self, uri: Url) {
        {
            let mut docs = self.documents.write().await;
            docs.close(&uri);
        }
        {
            let mut diags = self.diagnostics.write().await;
            diags.elements.insert(uri.clone(), vec![]);
        }
        self.client.publish_diagnostics(uri, vec![], None).await;
    }
}
