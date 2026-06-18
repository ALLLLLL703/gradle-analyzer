//! Tolerant dual-DSL parser frontends over the shared syntax substrate.
//!
//! Declared empty in Task 1; implemented in Tasks 5 (Kotlin DSL) and 6 (Groovy DSL).
//!
//! Each frontend drives the shared [`crate::gradle::syntax::Parser`] to a tolerant
//! [`crate::gradle::syntax::Parse`] (green tree + typed error side table) for its DSL's
//! supported nucleus, degrading anything out-of-nucleus to a bounded
//! [`crate::gradle::syntax::SyntaxKind::OPAQUE`] node rather than aborting. No full
//! Kotlin/Groovy semantics live here (that is Task 7).
//!
//! [`kotlin`] handles `.gradle.kts` (Task 5); [`groovy`] handles `.gradle` (Task 6).

pub mod groovy;
pub mod kotlin;

pub use groovy::parse_groovy;
pub use kotlin::parse_kotlin;
