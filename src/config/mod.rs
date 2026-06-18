//! Configuration: typed, validated, hot-reloadable settings.
//!
//! Every mutable knob the server uses lives in [`GradleAnalyzerConfig`], loaded from an
//! optional workspace-local `gradle-analyzer.toml` and an optional user-level file with
//! documented precedence (workspace > user > built-in defaults). [`ConfigManager`] holds
//! the live snapshot behind a lock-free cell and supports atomic hot-reload; the
//! [`watcher`] module wires that to filesystem changes.

pub mod error;
pub mod loader;
pub mod manager;
pub mod model;
pub mod raw;
pub mod watcher;

pub use error::ConfigError;
pub use loader::ConfigSources;
pub use manager::ConfigManager;
pub use model::GradleAnalyzerConfig;
