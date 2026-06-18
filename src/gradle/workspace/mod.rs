//! Workspace and document model: classification, snapshots, root, and invalidation.
//!
//! This is Task 2's minimal "workspace view": before any parsing, the server must know
//! WHAT a file is, WHERE its workspace root is, what an OPEN document currently holds,
//! and WHAT a change forces it to recompute. The layer is deliberately
//! parser/sidecar-agnostic (it never references `syntax/` or `sidecar/` types):
//!
//! - [`kind`] — [`GradleFileKind`] + [`DslLanguage`] and the pure [`GradleFileKind::classify`]
//!   classifier (root/settings/subproject build, `buildSrc`, version catalog, unknown).
//! - [`root`] — [`detect_workspace_root`] (settings ancestor wins, else top-level build
//!   script; a nested `build.gradle*` never becomes its own root) → a [`WorkspaceRoot`].
//! - [`document`] — [`TrackedDocument`], an immutable `Arc<str>` text snapshot + version +
//!   uri + kind; old snapshots stay valid after a change.
//! - [`store`] — [`WorkspaceDocumentStore`], the open/change/close full-text-sync lifecycle.
//! - [`invalidation`] — [`ChangeTrigger`] (a superset of file kinds incl. wrapper files) and
//!   the pure [`invalidation_for`] contract → an [`InvalidationScope`]/[`InvalidationKind`].
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::workspace::{
//!     ChangeTrigger, GradleFileKind, InvalidationKind, invalidation_for,
//! };
//! use std::path::Path;
//!
//! let root = Path::new("/proj");
//! let catalog = GradleFileKind::classify(Path::new("/proj/gradle/libs.versions.toml"), root);
//! assert_eq!(catalog, GradleFileKind::VersionCatalog);
//!
//! let trigger = ChangeTrigger::for_path(Path::new("/proj/gradle/libs.versions.toml"), root);
//! assert_eq!(invalidation_for(trigger).kind(), InvalidationKind::WorkspaceSemantic);
//! ```

pub mod document;
pub mod invalidation;
pub mod kind;
pub mod root;
pub mod store;

pub use document::TrackedDocument;
pub use invalidation::{ChangeTrigger, InvalidationKind, InvalidationScope, invalidation_for};
pub use kind::{DslLanguage, GradleFileKind};
pub use root::{RootResolution, WorkspaceRoot, detect_workspace_root};
pub use store::WorkspaceDocumentStore;
