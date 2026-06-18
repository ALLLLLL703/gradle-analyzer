//! Internationalization boundary for all user-facing text.
//!
//! User-visible strings (status messages, diagnostics, error reasons) are addressed
//! through a typed [`MessageKey`] and rendered by the [`Translator`] from the English
//! [`catalog`]. This keeps translatable text in one place and out of business logic.
//! Technical `tracing` logs may stay plain English and need no key.

pub mod catalog;
pub mod key;
pub mod translator;

pub use key::MessageKey;
pub use translator::Translator;
