//! Cross-cutting reusable helpers.
//!
//! Centralizes utilities that don't belong to a single feature domain: configuration
//! [`paths`] resolution and the stdio JSON-RPC [`probe`] framing helper used by tests
//! and the quality probe suite. Domain-specific logic does NOT live here.

pub mod paths;
pub mod probe;
