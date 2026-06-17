use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRoot {
    pub path: PathBuf,
}

impl From<PathBuf> for WorkspaceRoot {
    fn from(path: PathBuf) -> Self {
        WorkspaceRoot { path }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum WorkspaceFileRole {
    RootSettings,
    RootBuild,
    NestedBuild,
    Unknown,
}
