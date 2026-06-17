use std::{fs, path::Path};

use crate::config::model::{
    GradleConfig, I18nConfig, LoggingConfig, LspConfig, RawConfig, RuntimeConfig,
};
use anyhow::Context;

pub fn load_runtime_config(path: &Path) -> anyhow::Result<RuntimeConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;

    let raw: RawConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;

    Ok(RuntimeConfig {
        lsp: LspConfig {
            max_file_size_kb: raw.lsp.max_file_size_kb.unwrap_or(512),
            enable_placeholder_diagnostics: raw.lsp.enable_placeholder_diagnostics.unwrap_or(true),
        },
        logging: LoggingConfig {
            level: raw.logging.level.unwrap_or_else(|| "info".to_string()),
            log_to_file: raw.logging.log_to_file.unwrap_or(true),
        },
        gradle: GradleConfig {
            scan_depth: raw.gradle.scan_depth.unwrap_or(8),
            enable_kotlin_dsl: raw.gradle.enable_kotlin_dsl.unwrap_or(true),
            enable_groovy_dsl: raw.gradle.enable_groovy_dsl.unwrap_or(true),
            root_scan_detph: raw.gradle.root_scan_depth.unwrap_or(8),
        },
        i18n: I18nConfig {
            default_locale: raw
                .i18n
                .default_locale
                .unwrap_or_else(|| "zh-CN".to_string()),
            fallback_locale: raw
                .i18n
                .fallback_locale
                .unwrap_or_else(|| "zh-CN".to_string()),
        },
    })
}
