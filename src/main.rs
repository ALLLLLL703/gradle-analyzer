//! Thin binary entry point for the gradle-analyzer language server.
//!
//! Bootstraps logging, then hands control to [`gradle_analyzer::app::Application`],
//! which owns the `tower-lsp` stdio server loop. All real logic lives in the library
//! crate so it stays testable.

use gradle_analyzer::app::{Application, logging};

#[tokio::main]
async fn main() {
    logging::init_tracing();
    Application::bootstrap().run().await;
}
