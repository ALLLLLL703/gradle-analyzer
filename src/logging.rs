use tracing_appender::non_blocking::WorkerGuard;

use crate::config::model::RuntimeConfig;

pub fn init_tracing_for_lsp(_config: &RuntimeConfig) -> WorkerGuard {
    let log_dir = shellexpand::full("~/.local/state/gradle-analyzer/")
        .map(|path| path.into_owned())
        .unwrap_or_else(|_| "log".to_string());

    let file_appender = tracing_appender::rolling::daily(log_dir, "log.log");

    let (_file_writer, guard) = tracing_appender::non_blocking(file_appender);

    guard
}
