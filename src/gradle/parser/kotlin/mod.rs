//! Tolerant Kotlin-DSL (`.gradle.kts`) frontend over the shared syntax substrate.
//!
//! A handwritten, resilient recursive-descent frontend that drives the shared
//! [`crate::gradle::syntax::Parser`] to a [`Parse`] (green tree + typed error side table)
//! for the SUPPORTED NUCLEUS of Gradle Kotlin build/settings scripts:
//!
//! - `import` / `package` headers,
//! - `plugins { }` / `pluginManagement { }` (incl. `id("..") version "..."`,
//!   `kotlin("jvm")`, `` `kotlin-dsl` ``),
//! - `repositories { }`, `dependencies { }` (string `implementation("g:a:v")` AND type-safe
//!   `implementation(libs.foo)` / `implementation(libs.bundles.x)`),
//! - `tasks.register<T>("n") { }`, `tasks.named<T>("n")`, `tasks { }`,
//! - assignments (`group = "..."`, `version = "..."`, `extra["x"] = "..."`),
//! - generic block/lambda calls and `libs.*` accessor paths.
//!
//! Everything OUTSIDE the nucleus (control flow, `fun`/`class` declarations, arbitrary
//! expressions) degrades into a bounded [`crate::gradle::syntax::SyntaxKind::OPAQUE`] node
//! via the substrate primitive — never an abort, never a flood of `MalformedBlock`. Typed
//! syntax errors are emitted only for genuinely malformed nucleus constructs (an unclosed
//! `{`/`(`/`[` anchors one `UnclosedBlock` to the last consumed token). No Kotlin semantics
//! live here (that is Task 7); raw parser strings stay internal and the substrate's
//! `SyntaxErrorKind -> MessageKey` mapping localizes diagnostics downstream.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::parser::parse_kotlin;
//! use gradle_analyzer::gradle::syntax::SyntaxNode;
//!
//! let source = "plugins {\n    kotlin(\"jvm\") version \"1.9.22\"\n}\n";
//! let parse = parse_kotlin(source);
//!
//! // Valid nucleus parses with zero errors and round-trips exactly.
//! assert!(parse.errors.is_empty());
//! assert_eq!(parse.text(), source);
//! assert_eq!(SyntaxNode::new_root(parse.green).text(), source);
//! ```

pub mod blocks;
pub mod expr;
pub mod kinds;
pub mod statement;

use crate::gradle::syntax::{Parse, Parser};

use statement::parse_statement;

/// Parses Kotlin-DSL `source` into a tolerant [`Parse`] (green tree + typed error table).
///
/// Never panics and always round-trips: the returned tree's text equals `source` byte for
/// byte. Out-of-nucleus constructs become bounded opaque nodes; only genuinely malformed
/// nucleus constructs add typed errors.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::parser::parse_kotlin;
///
/// let parse = parse_kotlin("dependencies {\n    implementation(\"a:b:1.0\")\n}\n");
/// assert!(parse.errors.is_empty());
/// ```
pub fn parse_kotlin(source: &str) -> Parse {
    Parser::new(source).parse_with(|p| {
        while !p.at_eof() {
            parse_statement(p);
        }
    })
}

#[cfg(test)]
mod tests;
