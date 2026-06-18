//! Line-delimited JSON framing for the sidecar IPC, with a max-message-size guard.
//!
//! Each frame is exactly one JSON object serialized onto a single `\n`-terminated line.
//! This mirrors what the real Gradle Tooling-API `BuildAction` writes to its child stdio
//! (Task 14). Because JSON escapes control characters, a newline embedded *inside* a JSON
//! string value is encoded as `\n` (two characters) and never splits a frame — proven in
//! the tests below.
//!
//! Decoding is fallible but never panics: an over-long line or a non-JSON line yields a
//! typed, recoverable [`FrameError`] so the client can degrade to the static tier.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A recoverable framing error encountered while encoding or decoding a line.
///
/// Both variants are recoverable: the caller maps them to
/// [`crate::gradle::sidecar::SidecarFailure::MalformedFrame`] and keeps the static tier
/// alive rather than aborting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The serialized or received frame exceeded `max_bytes`.
    #[error("frame of {len} bytes exceeds the {max} byte limit")]
    Oversized {
        /// The actual frame length in bytes.
        len: usize,
        /// The configured maximum.
        max: usize,
    },
    /// The line was not a single valid JSON object of the expected shape.
    #[error("frame is not valid JSON: {reason}")]
    NotJson {
        /// A short, log-grade decode reason.
        reason: String,
    },
}

impl FrameError {
    /// A short, stable tag (`oversized` / `not-json`) for status detail and logs.
    pub fn detail(&self) -> &'static str {
        match self {
            FrameError::Oversized { .. } => "oversized",
            FrameError::NotJson { .. } => "not-json",
        }
    }
}

/// Serializes `value` to a single JSON line (no trailing newline), guarding its size.
///
/// Returns [`FrameError::Oversized`] when the serialized form exceeds `max_bytes`, so an
/// accidentally huge outbound frame is caught before it hits the transport. The caller (or
/// the runner) appends the `\n` terminator when writing.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::sidecar::framing::to_line;
/// use serde_json::json;
///
/// let line = to_line(&json!({"type": "hello"}), 1024).unwrap();
/// assert!(!line.contains('\n'));
/// ```
pub fn to_line<T: Serialize>(value: &T, max_bytes: usize) -> Result<String, FrameError> {
    let encoded = serde_json::to_string(value).map_err(|e| FrameError::NotJson {
        reason: e.to_string(),
    })?;
    guard_size(encoded.len(), max_bytes)?;
    Ok(encoded)
}

/// Decodes a single received line into `T`, guarding its size first.
///
/// The trailing `\n` (and any `\r`) is trimmed before decoding. Returns
/// [`FrameError::Oversized`] when the line exceeds `max_bytes`, or [`FrameError::NotJson`]
/// when it is not valid JSON of the expected shape. Never panics.
pub fn decode_line<T: DeserializeOwned>(line: &str, max_bytes: usize) -> Result<T, FrameError> {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    guard_size(trimmed.len(), max_bytes)?;
    serde_json::from_str(trimmed).map_err(|e| FrameError::NotJson {
        reason: e.to_string(),
    })
}

/// Returns [`FrameError::Oversized`] when `len` exceeds `max_bytes`.
fn guard_size(len: usize, max_bytes: usize) -> Result<(), FrameError> {
    if len > max_bytes {
        return Err(FrameError::Oversized {
            len,
            max: max_bytes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Probe {
        text: String,
    }

    #[test]
    fn oversized_line_is_a_recoverable_frame_error_not_a_panic() {
        let big = "x".repeat(64);
        let probe = Probe { text: big };
        let err = to_line(&probe, 16).unwrap_err();
        assert!(matches!(err, FrameError::Oversized { .. }));
        assert_eq!(err.detail(), "oversized");
    }

    #[test]
    fn embedded_newline_in_a_json_string_does_not_break_framing() {
        // A real newline inside the value: serde escapes it, so the encoded line holds
        // zero raw '\n' bytes and the value survives a round-trip intact.
        let probe = Probe {
            text: "line one\nline two".to_string(),
        };
        let line = to_line(&probe, 4096).unwrap();
        assert!(
            !line.contains('\n'),
            "encoded frame must not contain a raw newline: {line:?}"
        );

        let decoded: Probe = decode_line(&line, 4096).unwrap();
        assert_eq!(decoded.text, "line one\nline two");
    }

    #[test]
    fn non_json_line_is_a_recoverable_frame_error() {
        let err = decode_line::<Probe>("this is not json", 4096).unwrap_err();
        assert!(matches!(err, FrameError::NotJson { .. }));
        assert_eq!(err.detail(), "not-json");
    }

    #[test]
    fn decode_trims_trailing_newline_before_parsing() {
        let decoded: Probe = decode_line("{\"text\":\"ok\"}\n", 4096).unwrap();
        assert_eq!(decoded.text, "ok");
    }

    #[test]
    fn oversized_received_line_is_guarded_by_max_bytes() {
        let line = "{\"text\":\"aaaaaaaaaaaaaaaaaaaa\"}";
        let err = decode_line::<Probe>(line, 8).unwrap_err();
        assert!(matches!(err, FrameError::Oversized { .. }));
    }
}
