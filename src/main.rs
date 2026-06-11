use tower_lsp::{
    Client, LanguageServer,
    lsp_types::{InitializeParams, InitializeResult},
};

pub struct Backend {
    client: Client,
}

#[tokio::main]
async fn main() {
    println!("Hello World");
}
