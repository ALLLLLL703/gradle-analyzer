//! The built-in English message catalog.
//!
//! The catalog maps each [`MessageKey`] to a template string. Templates may embed
//! positional placeholders `{0}`, `{1}`, ... which the [`crate::i18n::Translator`]
//! substitutes from caller-supplied arguments. Keeping the catalog in one place
//! makes it the single source of translatable text and the obvious extension point
//! for additional locales later.

use crate::i18n::key::MessageKey;

/// Resolves the English template for `key`, or `None` if the catalog has no entry.
///
/// A `None` result is intentionally handled by the translator as a key-name
/// fallback rather than an error, so a missing entry degrades gracefully.
///
/// # Example
///
/// ```
/// use gradle_analyzer::i18n::MessageKey;
/// use gradle_analyzer::i18n::catalog::english_template;
///
/// assert!(english_template(MessageKey::ServerInitialized).is_some());
/// ```
pub fn english_template(key: MessageKey) -> Option<&'static str> {
    let template = match key {
        MessageKey::ServerInitialized => "Gradle analyzer language server initialized.",
        MessageKey::ServerShutdown => "Gradle analyzer language server is shutting down.",
        MessageKey::ModelUnavailable => {
            "Advanced Gradle model is not available yet; static analysis remains active."
        }
        MessageKey::ConfigReadError => "Could not read configuration file '{0}': {1}",
        MessageKey::ConfigParseError => "Configuration file '{0}' is not valid TOML: {1}",
        MessageKey::ConfigValidationError => "Configuration value '{0}' is invalid: {1}",
        MessageKey::ConfigReloaded => "Reloaded configuration from '{0}'.",
        MessageKey::UntranslatedProbe => return None,
    };
    Some(template)
}
