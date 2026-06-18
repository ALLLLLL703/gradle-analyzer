//! Typed message keys for every user-facing, status, and diagnostic string.
//!
//! All text shown to a user (status messages, diagnostics, error reasons) must be
//! addressed through a [`MessageKey`] rather than an inline literal, so the catalog
//! stays the single source of translatable strings. Technical `tracing` logs may
//! remain plain English and do NOT need a key.

use std::fmt;

/// A stable, typed identifier for a translatable message.
///
/// Each variant maps to exactly one catalog entry and one canonical dotted name
/// (see [`MessageKey::canonical_name`]). The canonical name doubles as the
/// missing-key fallback rendered by the translator, so it never panics on a gap.
///
/// # Example
///
/// ```
/// use gradle_analyzer::i18n::MessageKey;
///
/// assert_eq!(MessageKey::ServerInitialized.canonical_name(), "lsp.server_initialized");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageKey {
    // --- LSP lifecycle / status ---
    /// Server finished the initialize handshake.
    ServerInitialized,
    /// Server is shutting down.
    ServerShutdown,
    /// The advanced (sidecar-backed) model is not yet available.
    ModelUnavailable,

    // --- Configuration ---
    /// A configuration file could not be read from disk.
    ConfigReadError,
    /// A configuration file contained malformed TOML.
    ConfigParseError,
    /// A configuration value failed validation.
    ConfigValidationError,
    /// The live configuration snapshot was reloaded.
    ConfigReloaded,

    /// Reserved key intentionally absent from the catalog.
    ///
    /// It models a key added to the enum before its catalog entry exists, and lets
    /// tests exercise the missing-key fallback as a real, observable path.
    #[doc(hidden)]
    UntranslatedProbe,
}

impl MessageKey {
    /// Returns the stable dotted name for this key.
    ///
    /// This is what the translator falls back to when a catalog entry is missing,
    /// guaranteeing a non-panicking, greppable result.
    pub const fn canonical_name(self) -> &'static str {
        match self {
            MessageKey::ServerInitialized => "lsp.server_initialized",
            MessageKey::ServerShutdown => "lsp.server_shutdown",
            MessageKey::ModelUnavailable => "lsp.model_unavailable",
            MessageKey::ConfigReadError => "config.read_error",
            MessageKey::ConfigParseError => "config.parse_error",
            MessageKey::ConfigValidationError => "config.validation_error",
            MessageKey::ConfigReloaded => "config.reloaded",
            MessageKey::UntranslatedProbe => "diag.untranslated_probe",
        }
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.canonical_name())
    }
}
