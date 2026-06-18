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
        MessageKey::SidecarWrapperMissing => {
            "Gradle wrapper not found; advanced model unavailable, static analysis continues."
        }
        MessageKey::SidecarWrapperNotExecutable => {
            "Gradle wrapper is not executable; advanced model unavailable, static analysis continues."
        }
        MessageKey::SidecarMissingJvm => {
            "No compatible JVM found for the Gradle sidecar; static analysis continues."
        }
        MessageKey::SidecarSyncFailure => {
            "Gradle sync failed ({0}); advanced model unavailable, static analysis continues."
        }
        MessageKey::SidecarTimeout => {
            "Gradle model request timed out after {0} ms; static analysis continues."
        }
        MessageKey::SidecarMalformedFrame => {
            "Received a malformed sidecar message ({0}); advanced model unavailable, static analysis continues."
        }
        MessageKey::SidecarSchemaMismatch => {
            "Sidecar protocol version {0} is unsupported; advanced model unavailable, static analysis continues."
        }
        MessageKey::SidecarCanceled => {
            "Gradle model request was canceled; static analysis continues."
        }
        MessageKey::SidecarStaleCache => {
            "Cached Gradle model is stale; advanced model unavailable until refresh, static analysis continues."
        }
        MessageKey::SyntaxMissingEquals => "Missing '=' in assignment.",
        MessageKey::SyntaxKeywordTypo => "'{0}' looks like a misspelled keyword.",
        MessageKey::SyntaxUnclosedBlock => "Block opened here is never closed.",
        MessageKey::SyntaxMalformedBlock => "Block contents are malformed.",
        MessageKey::SyntaxUnterminatedString => "String literal is not terminated.",
        MessageKey::SyntaxUnexpectedToken => "Unexpected '{0}'.",
        MessageKey::UntranslatedProbe => return None,
    };
    Some(template)
}
