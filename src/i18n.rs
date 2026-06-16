pub mod message;

use std::sync::Arc;

use self::message::{DiagnosticMessages, LogMessages, Messages};

#[derive(Clone, Debug)]
pub struct LangHelper {
    messages: Arc<Messages>,
}

impl LangHelper {
    pub fn new(messages: Messages) -> Self {
        Self {
            messages: Arc::new(messages),
        }
    }

    pub fn placeholder_diagnostic(&self) -> &str {
        &self.messages.diagnostic.placeholder_detected
    }

    pub fn document_opened(&self) -> &str {
        &self.messages.log.document_opened
    }

    pub fn document_changed(&self) -> &str {
        &self.messages.log.document_changed
    }

    pub fn document_closed(&self) -> &str {
        &self.messages.log.document_closed
    }
}

impl Default for LangHelper {
    fn default() -> Self {
        Self::new(Messages {
            diagnostic: DiagnosticMessages {
                placeholder_detected: "placeholder".to_string(),
                file_too_large: "file too large".to_string(),
            },
            log: LogMessages {
                server_started: "server started".to_string(),
                document_opened: "document opened".to_string(),
                document_changed: "document changed".to_string(),
                document_closed: "document closed".to_string(),
            },
        })
    }
}
