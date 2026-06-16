use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{manager::ConfigManager, model::RuntimeConfig};

pub fn init_tracing_for_lsp(config: &RuntimeConfig) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(
        shellexpand::full("~/.local/state/gradle-analyzer/")
            .map(|ok| ok.as_ref())
            .unwrap_or("log"),
        "log.log",
    );

    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
}
