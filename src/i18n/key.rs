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

    // --- Sidecar failure statuses ---
    // Each maps 1:1 to a `crate::gradle::sidecar::SidecarFailure` variant and renders a
    // distinct, user-facing status while the static tier stays live (degraded fallback).
    /// The Gradle wrapper script was not found in the workspace.
    SidecarWrapperMissing,
    /// The Gradle wrapper exists but is not executable.
    SidecarWrapperNotExecutable,
    /// No suitable JVM was found to launch the sidecar.
    SidecarMissingJvm,
    /// The Gradle sync / build action failed inside the sidecar.
    SidecarSyncFailure,
    /// A sidecar request exceeded its configured deadline.
    SidecarTimeout,
    /// A sidecar IPC frame could not be decoded (oversized or non-JSON).
    SidecarMalformedFrame,
    /// The sidecar spoke an incompatible protocol version.
    SidecarSchemaMismatch,
    /// A sidecar request was canceled before completion.
    SidecarCanceled,
    /// The cached sidecar model is stale and was rejected.
    SidecarStaleCache,

    // --- Syntax diagnostics (rendered by the Task 9 diagnostics layer) ---
    // Each maps 1:1 to a `crate::gradle::syntax::SyntaxErrorKind` variant so the tolerant
    // parser keeps raw English strings internal and the diagnostics surface stays localized.
    /// An assignment is missing its `=` operator.
    SyntaxMissingEquals,
    /// An identifier looks like a misspelled keyword.
    SyntaxKeywordTypo,
    /// A block was opened but never closed before end of input.
    SyntaxUnclosedBlock,
    /// A block was opened but its contents are malformed.
    SyntaxMalformedBlock,
    /// A string literal was never closed before end of line or input.
    SyntaxUnterminatedString,
    /// A token was encountered where the grammar did not expect one.
    SyntaxUnexpectedToken,

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
            MessageKey::SidecarWrapperMissing => "sidecar.wrapper_missing",
            MessageKey::SidecarWrapperNotExecutable => "sidecar.wrapper_not_executable",
            MessageKey::SidecarMissingJvm => "sidecar.missing_jvm",
            MessageKey::SidecarSyncFailure => "sidecar.sync_failure",
            MessageKey::SidecarTimeout => "sidecar.timeout",
            MessageKey::SidecarMalformedFrame => "sidecar.malformed_frame",
            MessageKey::SidecarSchemaMismatch => "sidecar.schema_mismatch",
            MessageKey::SidecarCanceled => "sidecar.canceled",
            MessageKey::SidecarStaleCache => "sidecar.stale_cache",
            MessageKey::SyntaxMissingEquals => "syntax.missing_equals",
            MessageKey::SyntaxKeywordTypo => "syntax.keyword_typo",
            MessageKey::SyntaxUnclosedBlock => "syntax.unclosed_block",
            MessageKey::SyntaxMalformedBlock => "syntax.malformed_block",
            MessageKey::SyntaxUnterminatedString => "syntax.unterminated_string",
            MessageKey::SyntaxUnexpectedToken => "syntax.unexpected_token",
            MessageKey::UntranslatedProbe => "diag.untranslated_probe",
        }
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.canonical_name())
    }
}
