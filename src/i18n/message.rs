use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Messages {
    pub diagnostic: DiagnosticMessages,
    pub log: LogMessages,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiagnosticMessages {
    pub placeholder_detected: String,
    pub file_too_large: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct LogMessages {
    pub server_started: String,
    pub document_opened: String,
    pub document_changed: String,
    pub document_closed: String,
}
