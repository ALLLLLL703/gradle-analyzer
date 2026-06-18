//! Centralized path resolution for configuration files.
//!
//! Keeps the workspace-local and user-level config path conventions in one place so no
//! other module hardcodes them. The user-level path follows the XDG base-directory
//! spec (`$XDG_CONFIG_HOME`, else `$HOME/.config`) with a Windows `%APPDATA%` fallback.

use std::path::{Path, PathBuf};

/// The fixed config file name, shared by the workspace-local and user-level locations.
pub const CONFIG_FILE_NAME: &str = "gradle-analyzer.toml";

/// Returns the workspace-local config path: `<workspace_root>/gradle-analyzer.toml`.
///
/// # Example
///
/// ```
/// use gradle_analyzer::util::paths::workspace_config_path;
/// use std::path::Path;
///
/// let p = workspace_config_path(Path::new("/proj"));
/// assert!(p.ends_with("gradle-analyzer.toml"));
/// ```
pub fn workspace_config_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(CONFIG_FILE_NAME)
}

/// Returns the user-level config path, or `None` if no base directory can be resolved.
///
/// Resolution order: `$XDG_CONFIG_HOME/gradle-analyzer/`, then `$HOME/.config/
/// gradle-analyzer/`, then `%APPDATA%\gradle-analyzer\` on Windows.
pub fn user_config_path() -> Option<PathBuf> {
    user_config_dir().map(|dir| dir.join(CONFIG_FILE_NAME))
}

/// Resolves the user-level config directory per the rules in [`user_config_path`].
fn user_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("gradle-analyzer"));
    }
    if let Some(home) = non_empty_env("HOME") {
        return Some(PathBuf::from(home).join(".config").join("gradle-analyzer"));
    }
    if let Some(appdata) = non_empty_env("APPDATA") {
        return Some(PathBuf::from(appdata).join("gradle-analyzer"));
    }
    None
}

/// Reads an environment variable, treating empty values as unset.
fn non_empty_env(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_path_appends_file_name() {
        let p = workspace_config_path(Path::new("/some/root"));
        assert_eq!(p, PathBuf::from("/some/root/gradle-analyzer.toml"));
    }

    #[test]
    fn user_dir_prefers_xdg_then_home() {
        // SAFETY: single-threaded test mutating only this test's view of env.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", "/xdg");
            std::env::set_var("HOME", "/home/u");
        }
        assert_eq!(
            user_config_path(),
            Some(PathBuf::from("/xdg/gradle-analyzer/gradle-analyzer.toml"))
        );

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert_eq!(
            user_config_path(),
            Some(PathBuf::from("/home/u/.config/gradle-analyzer/gradle-analyzer.toml"))
        );
    }
}
