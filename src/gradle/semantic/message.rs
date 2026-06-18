//! The i18n boundary for semantic-status text.
//!
//! Extraction itself records typed data (a [`CatalogResolution`], a [`FactStatus`]) and emits
//! plain technical `tracing`; this module is where that typed data is rendered to user-facing
//! strings through the [`Translator`]/[`MessageKey`] seam, so no English lives in the
//! extraction logic. Feature layers (Tasks 9-13) call these helpers to describe how a
//! `libs.*` accessor resolved or why a catalog is unavailable.

use crate::i18n::{MessageKey, Translator};

use super::facts::CatalogResolution;

/// Renders a localized description of how a catalog accessor resolved.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{describe_resolution, CatalogResolution};
/// use gradle_analyzer::i18n::Translator;
///
/// let tr = Translator::new();
/// let resolved = CatalogResolution::Resolved {
///     alias: "guava".into(),
///     coordinate: "com.google.guava:guava:33.0.0-jre".into(),
/// };
/// let text = describe_resolution(&tr, "libs.guava", &resolved);
/// assert!(text.contains("com.google.guava:guava:33.0.0-jre"));
/// ```
pub fn describe_resolution(
    translator: &Translator,
    accessor: &str,
    resolution: &CatalogResolution,
) -> String {
    match resolution {
        CatalogResolution::Resolved { coordinate, .. } => {
            translator.get_text(MessageKey::SemanticCatalogResolved, &[accessor, coordinate])
        }
        CatalogResolution::Unresolved => {
            translator.get_text(MessageKey::SemanticCatalogUnresolved, &[accessor])
        }
    }
}

/// Renders a localized message that a version catalog could not be parsed.
pub fn describe_catalog_parse_error(translator: &Translator, document: &str) -> String {
    translator.get_text(MessageKey::SemanticCatalogParseError, &[document])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_renders_the_coordinate() {
        let tr = Translator::new();
        let resolution = CatalogResolution::Resolved {
            alias: "guava".into(),
            coordinate: "com.google.guava:guava:33.0.0-jre".into(),
        };
        let text = describe_resolution(&tr, "libs.guava", &resolution);
        assert!(text.contains("libs.guava"));
        assert!(text.contains("com.google.guava:guava:33.0.0-jre"));
    }

    #[test]
    fn unresolved_and_parse_error_render_localized_text() {
        let tr = Translator::new();
        let unresolved = describe_resolution(&tr, "libs.nope", &CatalogResolution::Unresolved);
        assert!(unresolved.contains("libs.nope"));
        let parse_error = describe_catalog_parse_error(&tr, "gradle/libs.versions.toml");
        assert!(parse_error.contains("gradle/libs.versions.toml"));
    }
}
