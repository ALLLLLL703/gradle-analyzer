use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use crate::services::DiagnosticsService;

impl DiagnosticsService {
    pub async fn publish_placeholder_diagnostic(
        &self,
        client: &tower_lsp::Client,
        uri: &Url,
        text: &str,
    ) {
        let runtime_config = self.config.get_config().await;
        if !runtime_config.lsp.enable_placeholder_diagnostics {
            client.publish_diagnostics(uri.clone(), vec![], None).await;
            return;
        }

        let diagnostics = if text.contains("TODO_GRADLE_ERROR") {
            vec![Diagnostic {
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 18),
                },
                message: self.lang.placeholder_diagnostic().to_string(),
                severity: Some(DiagnosticSeverity::ERROR),
                ..Default::default()
            }]
        } else {
            vec![]
        };

        client.publish_diagnostics(uri.clone(), diagnostics, None).await;
    }
}
