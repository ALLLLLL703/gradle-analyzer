//! The sidecar failure taxonomy, [`SidecarFailure`].
//!
//! The advanced (sidecar-backed) tier may fail in many ways — a missing Gradle wrapper,
//! no JVM, a sync error, a timeout, a malformed frame, a protocol mismatch, cancellation,
//! or a stale cache. Every such outcome is a [`SidecarFailure`] that maps to a localizable
//! [`MessageKey`] status and *always* degrades to the static tier
//! ([`SidecarFailure::degraded_to_static`] is `true` for every variant). The static tier
//! must never block on or fail because of the sidecar, so this type never panics and never
//! escalates to an unrecoverable error.

use crate::i18n::{MessageKey, Translator};

/// A recoverable failure of the sidecar (advanced) tier.
///
/// Each variant carries enough detail for a localized status and a `tracing` log, and each
/// maps 1:1 to a distinct [`MessageKey`]. Construct the variant that matches the failure,
/// then render it for the user with [`SidecarFailure::status_message`].
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::sidecar::SidecarFailure;
/// use gradle_analyzer::i18n::{MessageKey, Translator};
///
/// let failure = SidecarFailure::SchemaMismatch { version: 9 };
/// assert_eq!(failure.message_key(), MessageKey::SidecarSchemaMismatch);
/// assert!(failure.degraded_to_static());
///
/// let tr = Translator::new();
/// assert!(failure.status_message(&tr).contains('9'));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarFailure {
    /// The Gradle wrapper script could not be located in the workspace.
    WrapperMissing,
    /// The Gradle wrapper exists but lacks execute permission.
    WrapperNotExecutable,
    /// No compatible JVM was available to launch the sidecar.
    MissingJvm,
    /// The Gradle sync / build action failed; `detail` is a short technical reason.
    SyncFailure {
        /// A short, log-grade reason for the sync failure.
        detail: String,
    },
    /// A request exceeded its configured deadline; `elapsed_ms` is that deadline.
    Timeout {
        /// The deadline (in milliseconds) that elapsed before a reply arrived.
        elapsed_ms: u64,
    },
    /// An IPC frame could not be decoded; `detail` names the framing problem.
    MalformedFrame {
        /// A short reason such as `oversized` or `not-json`.
        detail: String,
    },
    /// The sidecar advertised an incompatible protocol version.
    SchemaMismatch {
        /// The unsupported protocol version the sidecar chose.
        version: u32,
    },
    /// The request was canceled before a response arrived.
    Canceled,
    /// The cached model was stale and rejected (real freshness gating lands in Task 16).
    StaleCache,
}

impl SidecarFailure {
    /// Returns the [`MessageKey`] used to render this failure as user-facing status.
    ///
    /// The mapping is total and injective: every variant has a distinct key, so the UI
    /// surface can show a specific, localized status for each failure mode.
    pub fn message_key(&self) -> MessageKey {
        match self {
            SidecarFailure::WrapperMissing => MessageKey::SidecarWrapperMissing,
            SidecarFailure::WrapperNotExecutable => MessageKey::SidecarWrapperNotExecutable,
            SidecarFailure::MissingJvm => MessageKey::SidecarMissingJvm,
            SidecarFailure::SyncFailure { .. } => MessageKey::SidecarSyncFailure,
            SidecarFailure::Timeout { .. } => MessageKey::SidecarTimeout,
            SidecarFailure::MalformedFrame { .. } => MessageKey::SidecarMalformedFrame,
            SidecarFailure::SchemaMismatch { .. } => MessageKey::SidecarSchemaMismatch,
            SidecarFailure::Canceled => MessageKey::SidecarCanceled,
            SidecarFailure::StaleCache => MessageKey::SidecarStaleCache,
        }
    }

    /// Whether this failure degrades to the always-live static tier.
    ///
    /// This is `true` for every variant: a sidecar failure must never fail the static
    /// tier. The method exists so callers can assert the invariant and emit the
    /// `degraded_to_static` signal without matching each variant.
    pub fn degraded_to_static(&self) -> bool {
        true
    }

    /// Renders the localized, user-facing status string for this failure.
    ///
    /// Variants that carry detail substitute it into the template's positional argument,
    /// so the rendered status names the offending version, deadline, or reason.
    pub fn status_message(&self, translator: &Translator) -> String {
        let key = self.message_key();
        match self {
            SidecarFailure::SyncFailure { detail }
            | SidecarFailure::MalformedFrame { detail } => translator.get_text(key, &[detail]),
            SidecarFailure::Timeout { elapsed_ms } => {
                translator.get_text(key, &[&elapsed_ms.to_string()])
            }
            SidecarFailure::SchemaMismatch { version } => {
                translator.get_text(key, &[&version.to_string()])
            }
            _ => translator.text(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The full set of variants, used to assert key distinctness and degradation.
    fn all_variants() -> Vec<SidecarFailure> {
        vec![
            SidecarFailure::WrapperMissing,
            SidecarFailure::WrapperNotExecutable,
            SidecarFailure::MissingJvm,
            SidecarFailure::SyncFailure {
                detail: "compileJava failed".to_string(),
            },
            SidecarFailure::Timeout { elapsed_ms: 1500 },
            SidecarFailure::MalformedFrame {
                detail: "not-json".to_string(),
            },
            SidecarFailure::SchemaMismatch { version: 9 },
            SidecarFailure::Canceled,
            SidecarFailure::StaleCache,
        ]
    }

    #[test]
    fn every_variant_maps_to_a_distinct_nonempty_localized_status_and_degrades() {
        let translator = Translator::new();
        let mut seen_keys = HashSet::new();

        for failure in all_variants() {
            let key = failure.message_key();
            assert!(
                seen_keys.insert(key),
                "duplicate MessageKey {key} for {failure:?}"
            );

            let status = failure.status_message(&translator);
            assert!(!status.trim().is_empty(), "empty status for {failure:?}");
            assert_ne!(
                status,
                key.canonical_name(),
                "status for {failure:?} fell back to the canonical key name (missing catalog entry)"
            );

            assert!(
                failure.degraded_to_static(),
                "{failure:?} must degrade to static"
            );
        }

        assert_eq!(seen_keys.len(), 9, "expected nine distinct failure keys");
    }

    #[test]
    fn detail_bearing_variants_inject_their_detail_into_the_status() {
        let translator = Translator::new();

        let timeout = SidecarFailure::Timeout { elapsed_ms: 1500 }.status_message(&translator);
        assert!(timeout.contains("1500"), "timeout status: {timeout}");

        let mismatch =
            SidecarFailure::SchemaMismatch { version: 9 }.status_message(&translator);
        assert!(mismatch.contains('9'), "mismatch status: {mismatch}");

        let sync = SidecarFailure::SyncFailure {
            detail: "compileJava failed".to_string(),
        }
        .status_message(&translator);
        assert!(sync.contains("compileJava failed"), "sync status: {sync}");
    }
}
