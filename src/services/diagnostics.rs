use std::{collections::HashMap, sync::Arc};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use crate::services::Backend;

#[derive(Default, Debug)]
pub struct DiagnosticsManager {
    pub elements: HashMap<Url, Vec<Diagnostic>>,
}

impl Backend {
    pub async fn publish_placeholder_diagnostic(&self, uri: &Url, text: &str) {
        let diagnostics = if text.contains("TODO_GRADLE_ERROR") {
            vec![Diagnostic {
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 18),
                },
                message: "placeholder".to_string(),
                severity: Some(DiagnosticSeverity::ERROR),
                ..Default::default()
            }]
        } else {
            vec![]
        };
        {
            let mut diag = self.diagnostics.write().await;
            diag.elements.insert(uri.clone(), diagnostics.clone());
            self.client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;
        }
    }
}
