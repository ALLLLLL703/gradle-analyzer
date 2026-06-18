//! JVM Tooling-API sidecar contract and process orchestration.
//!
//! Declared empty in Task 1; the IPC contract + fake runner land in Task 4 and the real
//! JVM process in Task 14.
//!
//! TODO(Task 4): versioned line-delimited JSON IPC with capability negotiation, a
//! max-message-size guard, malformed-frame recovery, a `ProcessRunner` trait with a fake
//! in-process implementation, and the failure taxonomy mapped to i18n status keys.
//! TODO(Task 14): real Gradle wrapper child process speaking that contract. The static
//! tier must never block on or fail because of the sidecar.
