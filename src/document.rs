use std::collections::HashMap;

use tower_lsp::lsp_types::Url;

pub struct DocumentStore {
    pub documents: HashMap<Url, DocumentSnapShot>,
}

pub struct DocumentSnapShot {
    pub version: i32,
    pub text: String,
}

impl DocumentStore {
    pub fn open(&mut self, uri: &Url, doc: DocumentSnapShot) {}

    pub fn update(&mut self, uri: &Url, doc: DocumentSnapShot) {}

    pub fn close(&mut self, uri: &Url) {}
    pub fn get(self, uri: &Url) -> Option<&DocumentSnapShot> {
        Default::default()
    }
}
