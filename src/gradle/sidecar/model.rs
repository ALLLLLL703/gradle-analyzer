//! The [`SidecarModel`]: the project model the real Gradle Tooling-API `BuildAction` will
//! emit (designed now, populated by Task 14).
//!
//! This is the serde shape of the advanced-tier payload — applied plugins, extension DSL
//! blocks (e.g. `dotnet {}`), task types, the resolved classpath and source jars, and the
//! version catalog. Task 4 fixes the contract and proves it round-trips over the wire; the
//! real values arrive when the JVM sidecar lands. Every field defaults to empty so a
//! partial model from an older sidecar still deserializes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The full Gradle project model imported from the sidecar.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::sidecar::model::{ExtensionInfo, SidecarModel};
///
/// let json = r#"{
///   "gradleVersion": "8.10",
///   "appliedPlugins": [],
///   "extensions": [{"name": "dotnet", "typeFqn": "com.example.DotnetExtension"}],
///   "taskTypes": [],
///   "classpathJars": [],
///   "sourceJars": [],
///   "versionCatalog": {"libraries": {}, "bundles": {}, "versions": {}, "plugins": {}}
/// }"#;
/// let model: SidecarModel = serde_json::from_str(json).unwrap();
/// assert_eq!(model.extensions[0].name, "dotnet");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarModel {
    /// The Gradle version that produced this model.
    #[serde(default)]
    pub gradle_version: String,
    /// Plugins applied to the build, by id and implementing class.
    #[serde(default)]
    pub applied_plugins: Vec<AppliedPlugin>,
    /// Extension DSL blocks contributed by plugins (e.g. `dotnet {}`).
    #[serde(default)]
    pub extensions: Vec<ExtensionInfo>,
    /// Task types registered in the build.
    #[serde(default)]
    pub task_types: Vec<TaskType>,
    /// Resolved classpath jar paths.
    #[serde(default)]
    pub classpath_jars: Vec<String>,
    /// Resolved `-sources.jar` paths for navigation.
    #[serde(default)]
    pub source_jars: Vec<String>,
    /// The parsed version catalog (`gradle/libs.versions.toml`).
    #[serde(default)]
    pub version_catalog: VersionCatalog,
}

/// A plugin applied to the build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPlugin {
    /// The plugin id (e.g. `org.jetbrains.kotlin.jvm`).
    pub id: String,
    /// The fully-qualified implementing class, if known.
    #[serde(default)]
    pub plugin_class: String,
}

/// A plugin-contributed extension DSL block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    /// The DSL block name as written in the build script (e.g. `dotnet`).
    pub name: String,
    /// The fully-qualified type backing the extension.
    pub type_fqn: String,
}

/// A task type registered in the build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskType {
    /// The task name or type name.
    pub name: String,
    /// The fully-qualified task class.
    pub type_fqn: String,
}

/// The Gradle version catalog model parsed from `libs.versions.toml`.
///
/// Maps are keyed by alias so accessor expressions (`libs.*`, `libs.bundles.*`,
/// `libs.plugins.*`) resolve against them in a later task.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionCatalog {
    /// `[libraries]` entries, by alias, rendered as a coordinate string.
    #[serde(default)]
    pub libraries: BTreeMap<String, String>,
    /// `[bundles]` entries, by alias, listing member library aliases.
    #[serde(default)]
    pub bundles: BTreeMap<String, Vec<String>>,
    /// `[versions]` entries, by alias.
    #[serde(default)]
    pub versions: BTreeMap<String, String>,
    /// `[plugins]` entries, by alias, rendered as a coordinate string.
    #[serde(default)]
    pub plugins: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a representative non-empty model including a `dotnet` extension block.
    fn sample_model() -> SidecarModel {
        let mut versions = BTreeMap::new();
        versions.insert("kotlin".to_string(), "2.0.0".to_string());

        let mut libraries = BTreeMap::new();
        libraries.insert(
            "guava".to_string(),
            "com.google.guava:guava:33.0.0-jre".to_string(),
        );

        let mut bundles = BTreeMap::new();
        bundles.insert("core".to_string(), vec!["guava".to_string()]);

        SidecarModel {
            gradle_version: "8.10".to_string(),
            applied_plugins: vec![AppliedPlugin {
                id: "org.jetbrains.kotlin.jvm".to_string(),
                plugin_class: "org.jetbrains.kotlin.gradle.plugin.KotlinPluginWrapper"
                    .to_string(),
            }],
            extensions: vec![ExtensionInfo {
                name: "dotnet".to_string(),
                type_fqn: "com.example.gradle.DotnetExtension".to_string(),
            }],
            task_types: vec![TaskType {
                name: "compileDotnet".to_string(),
                type_fqn: "com.example.gradle.CompileDotnet".to_string(),
            }],
            classpath_jars: vec!["/repo/.gradle/guava.jar".to_string()],
            source_jars: vec!["/repo/.gradle/guava-sources.jar".to_string()],
            version_catalog: VersionCatalog {
                libraries,
                bundles,
                versions,
                plugins: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn full_model_round_trips_through_json_intact() {
        let model = sample_model();
        let line = serde_json::to_string(&model).unwrap();
        let decoded: SidecarModel = serde_json::from_str(&line).unwrap();
        assert_eq!(decoded, model);

        let dotnet = decoded
            .extensions
            .iter()
            .find(|e| e.name == "dotnet")
            .expect("dotnet extension present");
        assert_eq!(dotnet.type_fqn, "com.example.gradle.DotnetExtension");
    }

    #[test]
    fn missing_optional_fields_default_to_empty() {
        let decoded: SidecarModel =
            serde_json::from_str(r#"{"gradleVersion": "8.5"}"#).unwrap();
        assert_eq!(decoded.gradle_version, "8.5");
        assert!(decoded.applied_plugins.is_empty());
        assert!(decoded.extensions.is_empty());
        assert!(decoded.version_catalog.libraries.is_empty());
    }
}
