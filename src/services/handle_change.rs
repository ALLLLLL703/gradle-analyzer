use tower_lsp::lsp_types::DidChangeTextDocumentParams;

use crate::{
    document::DocumentSnapshot,
    services::{Backend, diagnostics::context::AnalysisContext},
    workspace::WorkspaceFileRole,
};

impl Backend {
    pub async fn handle_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let Some(change) = params.content_changes.into_iter().next() else {
            return;
        };

        let workspace_root = self
            .runtime
            .workspace
            .find_workspace_root(std::path::Path::new(uri.path()));

        let snapshot = DocumentSnapshot {
            uri: uri.clone(),
            version,
            text: change.text,
            kind: self.runtime.workspace.classify_file(&uri),
            workspace_root,
        };

        let workspace_role = snapshot
            .workspace_root
            .as_ref()
            .map(|root| {
                self.runtime.workspace.classify_workspace_role(
                    std::path::Path::new(uri.path()),
                    root,
                    snapshot.kind.clone(),
                )
            })
            .unwrap_or(WorkspaceFileRole::Unknown);

        let context = AnalysisContext {
            snapshot: snapshot.clone(),
            workspace_role,
        };

        self.runtime.documents.update(&uri, snapshot).await;
        self.runtime
            .diagnostics
            .publish_placeholder_diagnostic(&self.client, &context)
            .await;
        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                self.runtime.lang.document_changed(),
            )
            .await;
    }
}
