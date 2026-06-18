//! Application composition: logging bootstrap and the server entry points.
//!
//! [`Application`] is the composition root that builds shared services and owns the
//! `tower-lsp` server loop ([`Application::run`] for real stdio,
//! [`Application::run_with_io`] for testable in-memory IO). [`logging`] installs the
//! tracing subscriber.

pub mod application;
pub mod logging;

pub use application::Application;
