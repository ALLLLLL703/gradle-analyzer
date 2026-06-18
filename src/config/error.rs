//! Typed configuration errors carrying a localizable [`MessageKey`].
//!
//! Loading never panics on bad input. A read failure, a TOML parse failure, or a
//! validation failure each produce a [`ConfigError`] whose [`ConfigError::message_key`]
//! lets the caller render a localized, user-facing explanation through the translator.

use std::path::PathBuf;

use crate::i18n::MessageKey;

/// An error encountered while reading, parsing, or validating configuration.
///
/// Each variant maps to a [`MessageKey`] so the user-facing surface stays localized
/// while the technical detail is preserved for logs.
///
/// # Example
///
/// ```
/// use gradle_analyzer::config::ConfigError;
/// use gradle_analyzer::i18n::MessageKey;
///
/// let err = ConfigError::Validation {
///     field: "watcher.debounce_ms".into(),
///     reason: "must be > 0".into(),
/// };
/// assert_eq!(err.message_key(), MessageKey::ConfigValidationError);
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A configuration file existed but could not be read.
    #[error("failed to read config file '{path}': {source}")]
    Read {
        /// The offending path.
        path: PathBuf,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// A configuration file contained malformed TOML.
    #[error("failed to parse config file '{path}': {source}")]
    Parse {
        /// The offending path.
        path: PathBuf,
        /// The underlying TOML parse error.
        source: toml::de::Error,
    },

    /// A merged configuration value failed validation.
    #[error("invalid config value for '{field}': {reason}")]
    Validation {
        /// The dotted field path that failed.
        field: String,
        /// A human-oriented reason.
        reason: String,
    },
}

impl ConfigError {
    /// Returns the [`MessageKey`] used to render this error for a user.
    pub fn message_key(&self) -> MessageKey {
        match self {
            ConfigError::Read { .. } => MessageKey::ConfigReadError,
            ConfigError::Parse { .. } => MessageKey::ConfigParseError,
            ConfigError::Validation { .. } => MessageKey::ConfigValidationError,
        }
    }
}
