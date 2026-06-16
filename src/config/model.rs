use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct RawConfig {
    pub lsp: RawLspConfig,
    pub logging: RawLoggingConfig,
    pub gradle: RawGradleConfig,
    pub i18n: RawI18nConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct RawLspConfig {
    pub max_file_size_kb: Option<usize>,
    pub enable_placeholder_diagnostics: Option<bool>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct RawLoggingConfig {
    pub level: Option<String>,
    pub log_to_file: Option<bool>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct RawGradleConfig {
    pub scan_depth: Option<usize>,
    pub enable_kotlin_dsl: Option<bool>,
    pub enable_groovy_dsl: Option<bool>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct RawI18nConfig {
    pub default_locale: Option<String>,
    pub fallback_locale: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub lsp: LspConfig,
    pub logging: LoggingConfig,
    pub gradle: GradleConfig,
    pub i18n: I18nConfig,
}

#[derive(Clone, Debug)]
pub struct LspConfig {
    pub max_file_size_kb: usize,
    pub enable_placeholder_diagnostics: bool,
}

#[derive(Clone, Debug)]
pub struct LoggingConfig {
    pub level: String,
    pub log_to_file: bool,
}

#[derive(Clone, Debug)]
pub struct GradleConfig {
    pub scan_depth: usize,
    pub enable_kotlin_dsl: bool,
    pub enable_groovy_dsl: bool,
}

#[derive(Clone, Debug)]
pub struct I18nConfig {
    pub default_locale: String,
    pub fallback_locale: String,
}
