//! Version-catalog (`gradle/libs.versions.toml`) parsing and accessor resolution.
//!
//! Gradle's central dependency declaration lives in a TOML catalog with four tables:
//! `[versions]`, `[libraries]`, `[bundles]`, and `[plugins]`. This module parses that TOML
//! (via the `toml` crate already in deps) into a [`VersionCatalog`] and resolves the
//! type-safe accessors build scripts use — `libs.foo`, `libs.bundles.x`, `libs.plugins.y` —
//! back to their catalog entry. An accessor with no matching entry resolves to
//! [`CatalogResolution::Unresolved`] (recorded, never a panic); a malformed/empty catalog
//! parses to an empty [`VersionCatalog`] carrying a parse-error flag.
//!
//! Accessor segments use Gradle's normalization: catalog aliases written with `-`/`_` are
//! addressed in code with `.`, so `commons-lang3` is reached as `libs.commons.lang3`. The
//! resolver therefore matches on the dotted, normalized alias form.

use std::collections::BTreeMap;

use super::facts::CatalogResolution;

/// A parsed Gradle version catalog: versions, libraries, bundles, and plugins.
///
/// All maps are keyed by the catalog alias in its dotted, normalized form (`-`/`_` -> `.`),
/// which is exactly the form a `libs.*` accessor uses, so resolution is a direct lookup.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::VersionCatalog;
///
/// let toml = r#"
/// [versions]
/// guava = "33.0.0-jre"
/// [libraries]
/// guava = { module = "com.google.guava:guava", version.ref = "guava" }
/// "#;
/// let catalog = VersionCatalog::parse(toml);
/// assert!(!catalog.had_parse_error());
/// assert_eq!(
///     catalog.resolve_library("libs.guava").unwrap(),
///     "com.google.guava:guava:33.0.0-jre"
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionCatalog {
    versions: BTreeMap<String, String>,
    libraries: BTreeMap<String, String>,
    bundles: BTreeMap<String, Vec<String>>,
    plugins: BTreeMap<String, CatalogPluginEntry>,
    /// Per-library normalized `version.ref` alias (when the entry used one).
    library_refs: BTreeMap<String, String>,
    /// Per-plugin normalized `version.ref` alias (when the entry used one).
    plugin_refs: BTreeMap<String, String>,
    had_parse_error: bool,
}

/// A `[plugins]` catalog entry: a plugin id plus an optional resolved version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPluginEntry {
    /// The plugin id (e.g. `org.jetbrains.kotlin.jvm`).
    pub id: String,
    /// The resolved version, if any.
    pub version: Option<String>,
}

impl VersionCatalog {
    /// Parses a `libs.versions.toml` body into a catalog, tolerating malformed input.
    ///
    /// On a TOML parse error the result is an empty catalog with [`VersionCatalog::had_parse_error`]
    /// set, so callers degrade gracefully (zero catalog facts) instead of panicking.
    pub fn parse(source: &str) -> VersionCatalog {
        let table: toml::Table = match source.parse() {
            Ok(table) => table,
            Err(_) => {
                return VersionCatalog {
                    had_parse_error: true,
                    ..VersionCatalog::default()
                };
            }
        };

        let versions = parse_versions(&table);
        let (libraries, library_refs) = parse_libraries(&table, &versions);
        let bundles = parse_bundles(&table);
        let (plugins, plugin_refs) = parse_plugins(&table, &versions);

        VersionCatalog {
            versions,
            libraries,
            bundles,
            plugins,
            library_refs,
            plugin_refs,
            had_parse_error: false,
        }
    }

    /// Returns `true` if the TOML failed to parse (the catalog is then empty).
    pub fn had_parse_error(&self) -> bool {
        self.had_parse_error
    }

    /// Returns the version-alias map (`alias -> version string`).
    pub fn versions(&self) -> &BTreeMap<String, String> {
        &self.versions
    }

    /// Returns the library-alias map (`alias -> coordinate string`).
    pub fn libraries(&self) -> &BTreeMap<String, String> {
        &self.libraries
    }

    /// Returns the bundle-alias map (`alias -> member aliases`).
    pub fn bundles(&self) -> &BTreeMap<String, Vec<String>> {
        &self.bundles
    }

    /// Returns the plugin-alias map (`alias -> id+version`).
    pub fn plugins(&self) -> &BTreeMap<String, CatalogPluginEntry> {
        &self.plugins
    }

    /// Returns the `[versions]` alias a library entry referenced via `version.ref`, if any.
    pub fn library_version_ref(&self, alias: &str) -> Option<&str> {
        self.library_refs.get(alias).map(String::as_str)
    }

    /// Returns the `[versions]` alias a plugin entry referenced via `version.ref`, if any.
    pub fn plugin_version_ref(&self, alias: &str) -> Option<&str> {
        self.plugin_refs.get(alias).map(String::as_str)
    }

    /// Folds another catalog's entries into this one (later wins on a key clash).
    ///
    /// Used to combine multiple `*.versions.toml` files into the single catalog that build
    /// scripts resolve `libs.*` accessors against.
    pub(crate) fn merge(&mut self, other: &VersionCatalog) {
        self.versions.extend(other.versions.clone());
        self.libraries.extend(other.libraries.clone());
        self.bundles.extend(other.bundles.clone());
        self.plugins.extend(other.plugins.clone());
        self.library_refs.extend(other.library_refs.clone());
        self.plugin_refs.extend(other.plugin_refs.clone());
    }

    /// Resolves a `libs.<alias>` accessor to its coordinate, or `None` if undefined.
    pub fn resolve_library(&self, accessor: &str) -> Option<&str> {
        let alias = accessor_alias(accessor, None)?;
        self.libraries.get(&alias).map(String::as_str)
    }

    /// Resolves any `libs.*` accessor to a [`CatalogResolution`] (libraries, bundles, plugins).
    ///
    /// Recognizes `libs.bundles.*` and `libs.plugins.*` prefixes, falling back to a library
    /// lookup. An accessor with no matching entry yields [`CatalogResolution::Unresolved`].
    pub fn resolve_accessor(&self, accessor: &str) -> CatalogResolution {
        if let Some(alias) = accessor_alias(accessor, Some("bundles")) {
            return match self.bundles.get(&alias) {
                Some(members) => CatalogResolution::Resolved {
                    alias,
                    coordinate: members.join(", "),
                },
                None => CatalogResolution::Unresolved,
            };
        }
        if let Some(alias) = accessor_alias(accessor, Some("plugins")) {
            return match self.plugins.get(&alias) {
                Some(entry) => CatalogResolution::Resolved {
                    alias,
                    coordinate: plugin_coordinate(entry),
                },
                None => CatalogResolution::Unresolved,
            };
        }
        match accessor_alias(accessor, None).and_then(|a| {
            self.libraries.get(&a).map(|coord| (a, coord.clone()))
        }) {
            Some((alias, coordinate)) => CatalogResolution::Resolved { alias, coordinate },
            None => CatalogResolution::Unresolved,
        }
    }
}

/// Renders a plugin entry as an `id:version` coordinate (id alone if version is absent).
fn plugin_coordinate(entry: &CatalogPluginEntry) -> String {
    match &entry.version {
        Some(version) => format!("{}:{}", entry.id, version),
        None => entry.id.clone(),
    }
}

/// Extracts the dotted alias from a `libs[.<sub>].<alias>` accessor.
///
/// With `sub = None`, drops only the leading catalog segment (`libs`). With `sub = Some("bundles")`,
/// requires and drops `libs.bundles`, returning `None` if the prefix is absent. The remaining
/// dotted segments form the normalized alias.
fn accessor_alias(accessor: &str, sub: Option<&str>) -> Option<String> {
    let mut segments = accessor.split('.');
    let _catalog = segments.next()?; // leading `libs` (or any catalog name)
    match sub {
        Some(expected) => {
            if segments.next()? != expected {
                return None;
            }
        }
        None => {
            // A bare `libs.bundles.*`/`libs.plugins.*` is NOT a plain library accessor.
        }
    }
    let rest: Vec<&str> = segments.collect();
    if rest.is_empty() {
        return None;
    }
    if sub.is_none() && matches!(rest.first(), Some(&"bundles") | Some(&"plugins")) {
        return None;
    }
    Some(rest.join("."))
}

/// Parses the `[versions]` table into `alias -> version` (dotted, normalized aliases).
fn parse_versions(table: &toml::Table) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(versions) = table.get("versions").and_then(toml::Value::as_table) else {
        return out;
    };
    for (alias, value) in versions {
        if let Some(version) = value.as_str() {
            out.insert(normalize_alias(alias), version.to_string());
        } else if let Some(ver) = value.as_table().and_then(|t| t.get("require")).and_then(toml::Value::as_str) {
            out.insert(normalize_alias(alias), ver.to_string());
        }
    }
    out
}

/// Parses the `[libraries]` table into `alias -> coordinate` plus captured `version.ref`s.
fn parse_libraries(
    table: &toml::Table,
    versions: &BTreeMap<String, String>,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut coords = BTreeMap::new();
    let mut refs = BTreeMap::new();
    let Some(libraries) = table.get("libraries").and_then(toml::Value::as_table) else {
        return (coords, refs);
    };
    for (alias, value) in libraries {
        if let Some(coord) = library_coordinate(value, versions) {
            let normalized = normalize_alias(alias);
            if let Some(reference) = version_ref_of(value) {
                refs.insert(normalized.clone(), normalize_alias(&reference));
            }
            coords.insert(normalized, coord);
        }
    }
    (coords, refs)
}

/// Returns the inline `version.ref` alias of a table-form entry, if present.
fn version_ref_of(value: &toml::Value) -> Option<String> {
    let table = value.as_table()?;
    match table.get("version") {
        Some(toml::Value::Table(inner)) => {
            inner.get("ref").and_then(toml::Value::as_str).map(str::to_string)
        }
        _ => None,
    }
}

/// Resolves one `[libraries]` entry to a `group:name:version` coordinate.
///
/// Handles the shorthand string form (`"g:a:v"`) and the table form with `module`/`group`+`name`
/// and `version`/`version.ref`. A missing version yields a `group:name` coordinate.
fn library_coordinate(value: &toml::Value, versions: &BTreeMap<String, String>) -> Option<String> {
    if let Some(shorthand) = value.as_str() {
        return Some(shorthand.to_string());
    }
    let table = value.as_table()?;
    let module = module_of(table)?;
    match version_of(table, versions) {
        Some(version) => Some(format!("{module}:{version}")),
        None => Some(module),
    }
}

/// Extracts the `group:name` module from a library table (`module` or `group`+`name`).
fn module_of(table: &toml::Table) -> Option<String> {
    if let Some(module) = table.get("module").and_then(toml::Value::as_str) {
        return Some(module.to_string());
    }
    let group = table.get("group").and_then(toml::Value::as_str)?;
    let name = table.get("name").and_then(toml::Value::as_str)?;
    Some(format!("{group}:{name}"))
}

/// Resolves a library/plugin `version` field: inline string or a `version.ref` lookup.
fn version_of(table: &toml::Table, versions: &BTreeMap<String, String>) -> Option<String> {
    match table.get("version") {
        Some(toml::Value::String(version)) => Some(version.clone()),
        Some(toml::Value::Table(inner)) => {
            let reference = inner.get("ref").and_then(toml::Value::as_str)?;
            versions.get(&normalize_alias(reference)).cloned()
        }
        _ => None,
    }
}

/// Parses the `[bundles]` table into `alias -> member aliases`.
fn parse_bundles(table: &toml::Table) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(bundles) = table.get("bundles").and_then(toml::Value::as_table) else {
        return out;
    };
    for (alias, value) in bundles {
        if let Some(array) = value.as_array() {
            let members = array
                .iter()
                .filter_map(toml::Value::as_str)
                .map(normalize_alias)
                .collect();
            out.insert(normalize_alias(alias), members);
        }
    }
    out
}

/// Parses the `[plugins]` table into `alias -> id+version` plus captured `version.ref`s.
fn parse_plugins(
    table: &toml::Table,
    versions: &BTreeMap<String, String>,
) -> (BTreeMap<String, CatalogPluginEntry>, BTreeMap<String, String>) {
    let mut entries = BTreeMap::new();
    let mut refs = BTreeMap::new();
    let Some(plugins) = table.get("plugins").and_then(toml::Value::as_table) else {
        return (entries, refs);
    };
    for (alias, value) in plugins {
        if let Some(entry) = plugin_entry(value, versions) {
            let normalized = normalize_alias(alias);
            if let Some(reference) = version_ref_of(value) {
                refs.insert(normalized.clone(), normalize_alias(&reference));
            }
            entries.insert(normalized, entry);
        }
    }
    (entries, refs)
}

/// Resolves one `[plugins]` entry to an id plus optional version.
fn plugin_entry(value: &toml::Value, versions: &BTreeMap<String, String>) -> Option<CatalogPluginEntry> {
    if let Some(shorthand) = value.as_str() {
        let (id, version) = split_plugin_shorthand(shorthand);
        return Some(CatalogPluginEntry { id, version });
    }
    let table = value.as_table()?;
    let id = table.get("id").and_then(toml::Value::as_str)?.to_string();
    let version = version_of(table, versions);
    Some(CatalogPluginEntry { id, version })
}

/// Splits a `"id:version"` plugin shorthand into its parts.
fn split_plugin_shorthand(shorthand: &str) -> (String, Option<String>) {
    match shorthand.split_once(':') {
        Some((id, version)) => (id.to_string(), Some(version.to_string())),
        None => (shorthand.to_string(), None),
    }
}

/// Normalizes a catalog alias to its dotted accessor form (`-`/`_` -> `.`).
fn normalize_alias(alias: &str) -> String {
    alias.replace(['-', '_'], ".")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"
[versions]
guava = "33.0.0-jre"
junit = "5.10.1"

[libraries]
guava = { module = "com.google.guava:guava", version.ref = "guava" }
junit-jupiter = { group = "org.junit.jupiter", name = "junit-jupiter", version.ref = "junit" }
commons = "org.apache.commons:commons-lang3:3.14.0"

[bundles]
networking = ["guava", "junit-jupiter"]

[plugins]
spotless = { id = "com.diffplug.spotless", version = "6.25.0" }
"#;

    #[test]
    fn resolves_library_with_version_ref() {
        let catalog = VersionCatalog::parse(CATALOG);
        assert!(!catalog.had_parse_error());
        assert_eq!(catalog.resolve_library("libs.guava"), Some("com.google.guava:guava:33.0.0-jre"));
    }

    #[test]
    fn resolves_group_name_library_and_normalized_alias() {
        let catalog = VersionCatalog::parse(CATALOG);
        // junit-jupiter alias is reached as libs.junit.jupiter (dotted, normalized).
        assert_eq!(
            catalog.resolve_library("libs.junit.jupiter"),
            Some("org.junit.jupiter:junit-jupiter:5.10.1")
        );
    }

    #[test]
    fn resolves_shorthand_string_library() {
        let catalog = VersionCatalog::parse(CATALOG);
        assert_eq!(catalog.resolve_library("libs.commons"), Some("org.apache.commons:commons-lang3:3.14.0"));
    }

    #[test]
    fn resolves_bundle_and_plugin_accessors() {
        let catalog = VersionCatalog::parse(CATALOG);
        let bundle = catalog.resolve_accessor("libs.bundles.networking");
        assert!(bundle.is_resolved(), "bundle resolves");
        let plugin = catalog.resolve_accessor("libs.plugins.spotless");
        match plugin {
            CatalogResolution::Resolved { coordinate, .. } => {
                assert_eq!(coordinate, "com.diffplug.spotless:6.25.0");
            }
            CatalogResolution::Unresolved => panic!("plugin accessor should resolve"),
        }
    }

    #[test]
    fn undefined_accessor_is_unresolved_not_a_panic() {
        let catalog = VersionCatalog::parse(CATALOG);
        assert_eq!(catalog.resolve_accessor("libs.nope"), CatalogResolution::Unresolved);
        assert_eq!(catalog.resolve_library("libs.nope"), None);
    }

    #[test]
    fn empty_and_garbage_catalog_degrade_without_panic() {
        let empty = VersionCatalog::parse("");
        assert!(!empty.had_parse_error(), "empty TOML is valid, just has no entries");
        assert!(empty.libraries().is_empty());

        let garbage = VersionCatalog::parse("this = = not valid toml [[[");
        assert!(garbage.had_parse_error(), "garbage flags a parse error");
        assert!(garbage.libraries().is_empty());
        assert_eq!(garbage.resolve_accessor("libs.guava"), CatalogResolution::Unresolved);
    }
}
