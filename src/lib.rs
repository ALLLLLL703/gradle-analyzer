//! gradle-analyzer: a production Gradle language server for Groovy and Kotlin builds.
//!
//! This crate is organized around bootstrap-first platform seams (Task 1 of the
//! `gradle-analyzer-lsp-v2` plan) that all later features build on:
//!
//! - [`app`]: composition root + the `tower-lsp` server loop ([`app::Application`]).
//! - [`config`]: typed, validated, hot-reloadable TOML configuration
//!   ([`config::ConfigManager`], [`config::GradleAnalyzerConfig`]).
//! - [`i18n`]: the [`i18n::Translator`]/[`i18n::MessageKey`] boundary for all
//!   user-facing text.
//! - [`lsp`]: the [`lsp::GradleLanguageServer`] protocol surface.
//! - [`gradle`]: analysis seams (syntax, parser, semantic, workspace, sidecar) declared
//!   here and filled by later tasks.
//! - [`services`]: background orchestration (later).
//! - [`util`]: cross-cutting helpers (config paths, the stdio probe).
//!
//! # Example
//!
//! ```no_run
//! # async fn run() {
//! use gradle_analyzer::app::Application;
//!
//! gradle_analyzer::app::logging::init_tracing();
//! Application::bootstrap().run().await;
//! # }
//! ```

pub mod app;
pub mod config;
pub mod gradle;
pub mod i18n;
pub mod lsp;
pub mod services;
pub mod util;
