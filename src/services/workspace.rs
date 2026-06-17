use std::{path::Path, sync::Arc};

use tower_lsp::lsp_types::Url;

use crate::{
    config::manager::ConfigManager,
    document::findkind::GradleFileKind,
    workspace::{WorkspaceFileRole, WorkspaceRoot},
};

pub struct WorkspaceService {
    config: Arc<ConfigManager>,
}

impl Default for WorkspaceService {
    fn default() -> Self {
        Self {
            config: ConfigManager::global(),
        }
    }
}

impl WorkspaceService {
    pub fn new(config: Arc<ConfigManager>) -> Self {
        Self { config }
    }

    pub fn classify_file(&self, uri: &Url) -> GradleFileKind {
        let filename = Path::new(uri.path())
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        classify_gradle_file_name(&filename)
    }

    pub fn find_workspace_root(&self, path: &Path) -> Option<WorkspaceRoot> {
        let max_depth = self.config.current_config_or_default().gradle.root_scan_detph;

        let mut current = if path.is_dir() { path } else { path.parent()? };

        for _depth in 0..=max_depth {
            if contains_workspace_settings_file(current) {
                return Some(WorkspaceRoot::from(current.to_path_buf()));
            }

            let parent = current.parent()?;
            current = parent;
        }

        None
    }

    pub fn classify_workspace_role(
        &self,
        file_path: &std::path::Path,
        root: &WorkspaceRoot,
        kind: GradleFileKind,
    ) -> WorkspaceFileRole {
        match kind {
            GradleFileKind::SettingsGroovy | GradleFileKind::SettingsKotlin => {
                if file_path.parent() == Some(root.path.as_path()) {
                    WorkspaceFileRole::RootSettings
                } else {
                    WorkspaceFileRole::Unknown
                }
            }
            GradleFileKind::BuildGroovy | GradleFileKind::BuildKotlin => {
                if file_path.parent() == Some(root.path.as_path()) {
                    WorkspaceFileRole::RootBuild
                } else {
                    WorkspaceFileRole::NestedBuild
                }
            }
            GradleFileKind::Unknown => WorkspaceFileRole::Unknown,
        }
    }
}

fn contains_workspace_settings_file(dir: &Path) -> bool {
    dir.join("settings.gradle").is_file() || dir.join("settings.gradle.kts").is_file()
}

pub fn classify_gradle_file_name(file_name: &str) -> GradleFileKind {
    match file_name {
        "settings.gradle" => GradleFileKind::SettingsGroovy,
        "settings.gradle.kts" => GradleFileKind::SettingsKotlin,
        "build.gradle" => GradleFileKind::BuildGroovy,
        "build.gradle.kts" => GradleFileKind::BuildKotlin,
        _ => GradleFileKind::Unknown,
    }
}
