//! Gradle language analysis modules.
//!
//! Groups the analysis pipeline declared as empty seams in Task 1 and filled by later
//! tasks: tolerant [`syntax`], the dual-DSL [`parser`], the static [`semantic`] graph,
//! the [`workspace`] document model, and the JVM [`sidecar`]. Task 1 only declares them
//! so later work attaches without restructuring.

pub mod parser;
pub mod completion;
pub mod diagnostics;
pub mod navigation;
pub mod semantic;
pub mod sidecar;
pub mod symbols;
pub mod syntax;
pub mod workspace;
