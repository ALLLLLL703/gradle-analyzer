//! Loading and merging configuration sources into a validated snapshot.
//!
//! [`ConfigSources`] names the optional workspace-local and user-level files. The
//! loader reads each present file, parses it as TOML (a missing file is skipped, a
//! malformed file is a typed [`ConfigError::Parse`]), merges them with workspace
//! winning over user winning over built-in defaults, then validates the result.

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info};

use crate::config::error::ConfigError;
use crate::config::model::GradleAnalyzerConfig;
use crate::config::raw::RawConfig;

/// The set of files that contribute to a configuration snapshot.
///
/// Either path may be `None` (absent), in which case that layer is skipped. Precedence
/// is fixed: workspace overrides user overrides built-in defaults.
///
/// # Example
///
/// ```no_run
/// use gradle_analyzer::config::ConfigSources;
/// use std::path::PathBuf;
///
/// let sources = ConfigSources {
///     user: None,
///     workspace: Some(PathBuf::from("gradle-analyzer.toml")),
/// };
/// let cfg = sources.load().expect("valid or absent config");
/// let _ = cfg.watcher.debounce_ms;
/// ```
#[derive(Debug, Clone, Default)]
pub struct ConfigSources {
    /// User-level override file (lower precedence).
    pub user: Option<PathBuf>,
    /// Workspace-local file (higher precedence).
    pub workspace: Option<PathBuf>,
}

impl ConfigSources {
    /// Convenience constructor for a workspace-only source set.
    pub fn workspace_only(path: impl Into<PathBuf>) -> Self {
        Self {
            user: None,
            workspace: Some(path.into()),
        }
    }

    /// Loads, merges (workspace > user > defaults), and validates into a snapshot.
    ///
    /// An absent file contributes nothing; a present-but-unreadable file is a
    /// [`ConfigError::Read`]; a present-but-malformed file is a [`ConfigError::Parse`].
    /// With no files present, built-in defaults are returned.
    pub fn load(&self) -> Result<GradleAnalyzerConfig, ConfigError> {
        let mut raw = RawConfig::default();

        if let Some(user_path) = &self.user {
            if let Some(user_raw) = read_optional_raw(user_path)? {
                debug!(path = %user_path.display(), "merged user-level config layer");
                raw = raw.merge(user_raw);
            }
        }

        if let Some(workspace_path) = &self.workspace {
            if let Some(workspace_raw) = read_optional_raw(workspace_path)? {
                debug!(path = %workspace_path.display(), "merged workspace config layer");
                raw = raw.merge(workspace_raw);
            }
        }

        let config = raw.into_validated()?;
        info!(
            user = self.user.as_ref().map(|p| p.display().to_string()),
            workspace = self.workspace.as_ref().map(|p| p.display().to_string()),
            "loaded configuration snapshot"
        );
        Ok(config)
    }
}

/// Reads and parses one file, returning `None` if it does not exist.
fn read_optional_raw(path: &Path) -> Result<Option<RawConfig>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let raw: RawConfig = toml::from_str(&content).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ga-cfg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        path
    }

    #[test]
    fn absent_config_yields_valid_defaults() {
        let sources = ConfigSources::workspace_only(temp_dir().join("does-not-exist.toml"));
        let cfg = sources.load().expect("absent file must be valid");
        assert_eq!(cfg, GradleAnalyzerConfig::default());
    }

    #[test]
    fn malformed_toml_returns_typed_parse_error_with_message_key() {
        let dir = temp_dir();
        let bad = write_file(&dir, "gradle-analyzer.toml", "this is = = not toml [[[");
        let sources = ConfigSources::workspace_only(bad);

        let err = sources.load().expect_err("malformed TOML must error, not panic");
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert_eq!(
            err.message_key(),
            crate::i18n::MessageKey::ConfigParseError
        );
    }

    #[test]
    fn out_of_range_value_returns_typed_validation_error() {
        let dir = temp_dir();
        let bad = write_file(
            &dir,
            "gradle-analyzer.toml",
            "[watcher]\ndebounce_ms = 0\n",
        );
        let sources = ConfigSources::workspace_only(bad);

        let err = sources.load().expect_err("zero debounce must fail validation");
        assert!(matches!(err, ConfigError::Validation { .. }));
        assert_eq!(
            err.message_key(),
            crate::i18n::MessageKey::ConfigValidationError
        );
    }

    #[test]
    fn workspace_overrides_user_overrides_default() {
        let dir = temp_dir();
        // Default debounce is 250. User sets 400, workspace sets 600.
        let user = write_file(&dir, "user.toml", "[watcher]\ndebounce_ms = 400\n");
        let workspace = write_file(&dir, "workspace.toml", "[watcher]\ndebounce_ms = 600\n");

        // Workspace beats user.
        let both = ConfigSources {
            user: Some(user.clone()),
            workspace: Some(workspace),
        };
        assert_eq!(both.load().unwrap().watcher.debounce_ms, 600);

        // User beats default when workspace is silent on the key.
        let user_only = ConfigSources {
            user: Some(user),
            workspace: Some(dir.join("missing-workspace.toml")),
        };
        assert_eq!(user_only.load().unwrap().watcher.debounce_ms, 400);
    }
}
