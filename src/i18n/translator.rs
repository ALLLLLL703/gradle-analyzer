//! The [`Translator`]: the single rendering entry point for user-facing text.
//!
//! Every status, diagnostic, and error string shown to a user flows through
//! [`Translator::get_text`]. The translator renders the English catalog template
//! for a [`MessageKey`], substituting positional `{0}`, `{1}`, ... placeholders from
//! caller-supplied arguments. A missing catalog entry falls back to the key's
//! canonical name and never panics.

use crate::i18n::catalog::english_template;
use crate::i18n::key::MessageKey;

/// Renders user-facing messages from typed [`MessageKey`]s.
///
/// The translator is cheap to clone and holds no mutable state, so it can be shared
/// freely across async tasks. Locale selection is intentionally minimal for now (a
/// single English catalog); the type is the seam where additional locales attach.
///
/// # Example
///
/// ```
/// use gradle_analyzer::i18n::{MessageKey, Translator};
///
/// let tr = Translator::new();
/// let msg = tr.get_text(MessageKey::ConfigReloaded, &["gradle-analyzer.toml"]);
/// assert_eq!(msg, "Reloaded configuration from 'gradle-analyzer.toml'.");
/// ```
#[derive(Debug, Clone, Default)]
pub struct Translator {
    _private: (),
}

impl Translator {
    /// Creates a translator backed by the built-in English catalog.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Renders `key` with positional `args` substituted for `{0}`, `{1}`, ... .
    ///
    /// If the catalog has no entry for `key`, the canonical key name is returned so
    /// the result is greppable and the call never panics. Extra args are ignored;
    /// placeholders with no matching arg are left intact.
    pub fn get_text(&self, key: MessageKey, args: &[&str]) -> String {
        match english_template(key) {
            Some(template) => render_template(template, args),
            None => key.canonical_name().to_string(),
        }
    }

    /// Convenience for messages that take no arguments.
    pub fn text(&self, key: MessageKey) -> String {
        self.get_text(key, &[])
    }
}

/// Substitutes positional `{n}` placeholders in `template` with `args[n]`.
///
/// Scans once; an unmatched placeholder (index out of range) is preserved verbatim,
/// which keeps malformed templates visible instead of silently dropping text.
fn render_template(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if ch != '{' {
            out.push(ch);
            continue;
        }

        // Collect the digits between '{' and '}'.
        let mut index_digits = String::new();
        let mut closed = false;
        while let Some(&(_, next)) = chars.peek() {
            if next == '}' {
                chars.next();
                closed = true;
                break;
            }
            if next.is_ascii_digit() {
                index_digits.push(next);
                chars.next();
            } else {
                break;
            }
        }

        match (closed, index_digits.parse::<usize>()) {
            (true, Ok(index)) if index < args.len() => out.push_str(args[index]),
            // Out-of-range or unparseable placeholder: keep it literal for visibility.
            (true, Ok(_)) => {
                out.push('{');
                out.push_str(&index_digits);
                out.push('}');
            }
            _ => {
                out.push('{');
                out.push_str(&index_digits);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_key_renders_english_with_arg_substitution() {
        let tr = Translator::new();
        let rendered = tr.get_text(MessageKey::ConfigReadError, &["build/x.toml", "no such file"]);
        assert_eq!(
            rendered,
            "Could not read configuration file 'build/x.toml': no such file"
        );
    }

    #[test]
    fn no_arg_key_renders_plain_text() {
        let tr = Translator::new();
        assert_eq!(
            tr.text(MessageKey::ServerInitialized),
            "Gradle analyzer language server initialized."
        );
    }

    #[test]
    fn missing_arg_leaves_placeholder_literal() {
        // ConfigReadError expects two args; supply zero -> placeholders stay literal.
        let tr = Translator::new();
        let rendered = tr.get_text(MessageKey::ConfigReadError, &[]);
        assert_eq!(rendered, "Could not read configuration file '{0}': {1}");
    }

    #[test]
    fn render_template_directly_handles_repeated_and_ordered_indices() {
        assert_eq!(render_template("{0}-{1}-{0}", &["a", "b"]), "a-b-a");
    }

    #[test]
    fn unknown_key_falls_back_to_canonical_name_without_panic() {
        let tr = Translator::new();
        let rendered = tr.get_text(MessageKey::UntranslatedProbe, &["ignored"]);
        assert_eq!(rendered, "diag.untranslated_probe");
    }
}
