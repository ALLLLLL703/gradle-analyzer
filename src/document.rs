pub mod findkind;

use std::collections::HashMap;

use tower_lsp::lsp_types::Url;

use crate::document::findkind::GradleFileKind;
use crate::workspace::WorkspaceRoot;

pub struct DocumentStore {
    pub documents: HashMap<Url, DocumentSnapshot>,
}

#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    pub uri: Url,
    pub version: i32,
    pub text: String,
    pub kind: GradleFileKind,
    pub workspace_root: Option<WorkspaceRoot>,
}

impl DocumentStore {
    pub fn open(&mut self, uri: &Url, doc: DocumentSnapshot) {
        self.documents.insert(uri.clone(), doc);
    }

    pub fn update(&mut self, uri: &Url, doc: DocumentSnapshot) {
        self.documents.insert(uri.clone(), doc);
    }

    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<&DocumentSnapshot> {
        self.documents.get(uri)
    }
}
