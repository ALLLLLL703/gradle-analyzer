use super::model::{GradleConfig, I18nConfig, LoggingConfig, LspConfig, RuntimeConfig};

pub fn default_runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        lsp: LspConfig {
            max_file_size_kb: 512,
            enable_placeholder_diagnostics: true,
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            log_to_file: true,
        },
        gradle: GradleConfig {
            scan_depth: 8,
            enable_kotlin_dsl: true,
            enable_groovy_dsl: true,

            root_scan_detph: 8,
        },
        i18n: I18nConfig {
            default_locale: "zh-CN".to_string(),
            fallback_locale: "zh-CN".to_string(),
        },
    }
}
