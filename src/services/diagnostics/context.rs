use crate::{document::DocumentSnapshot, workspace::WorkspaceFileRole};

#[derive(Debug)]
pub struct AnalysisContext {
    pub snapshot: DocumentSnapshot,
    pub workspace_role: WorkspaceFileRole,
}
