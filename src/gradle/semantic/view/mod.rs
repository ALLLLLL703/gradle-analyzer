//! A DSL-agnostic "statement view" over the two frontends' red trees.
//!
//! The Kotlin and Groovy grammars produce different node shapes (Kotlin wraps a call's head
//! in an `ACCESS_PATH` node; Groovy leads with a bare `IDENT` token), but every extractor
//! wants the same thing: a head path, a flat argument list, and an optional block body. This
//! module lowers BOTH trees into one normalized [`Statement`]/[`CallExpr`] shape so the
//! per-fact-kind extractors are written once, against the view, not per DSL.
//!
//! Lowering is deliberately shallow and tolerant: it walks only recognized nucleus nodes and
//! **skips `OPAQUE`/`ERROR_NODE` subtrees by design**, so malformed regions never reach an
//! extractor. A construct missing a modeled piece still lowers (e.g. a call with no args),
//! letting the extractor mark the resulting fact `Partial` rather than dropping it.

pub mod groovy;
pub mod kotlin;

use std::rc::Rc;

use crate::gradle::syntax::{SyntaxNode, TextSpan};
use crate::gradle::workspace::DslLanguage;

/// A normalized top-level-or-in-block statement, independent of DSL.
#[derive(Debug, Clone)]
pub enum Statement {
    /// A call/command (`plugins { }`, `implementation("x")`, `include(":app")`).
    Call(CallExpr),
    /// An assignment (`group = "..."`, `rootProject.name = "..."`).
    Assignment(AssignExpr),
    /// An `import` header.
    Import {
        /// The dotted import path (`org.gradle.api.Project`).
        path: String,
        /// The source span of the whole import.
        span: TextSpan,
    },
}

/// A normalized call: a dotted head, flat args, optional block, and plugin suffixes.
#[derive(Debug, Clone)]
pub struct CallExpr {
    /// The dotted head path (`tasks.register`, `id`, `mavenCentral`).
    pub head: String,
    /// A cleaned single-token head form for plugin-id fallback (backticks stripped).
    pub head_raw: String,
    /// The flattened argument list.
    pub args: Vec<ArgExpr>,
    /// The trailing block / closure body, if present.
    pub block: Option<Rc<SyntaxNode>>,
    /// Kotlin plugin infix suffixes (`version "x"`, `apply false`).
    pub suffixes: Vec<PluginSuffix>,
    /// The source span of the whole call.
    pub span: TextSpan,
}

impl CallExpr {
    /// Returns the first string argument, if any (the common plugin-id/coordinate slot).
    pub fn first_string(&self) -> Option<&str> {
        self.args.iter().find_map(|arg| match arg {
            ArgExpr::Str(text) => Some(text.as_str()),
            _ => None,
        })
    }

    /// Returns the value of a named argument by key (e.g. `group`, `name`, `version`).
    pub fn named(&self, key: &str) -> Option<&ArgExpr> {
        self.args.iter().find_map(|arg| match arg {
            ArgExpr::Named { name, value } if name == key => Some(value.as_ref()),
            _ => None,
        })
    }
}

/// A normalized assignment (`lhs = rhs`).
#[derive(Debug, Clone)]
pub struct AssignExpr {
    /// The dotted assignment target (`group`, `version`, `rootProject.name`).
    pub target: String,
    /// The right-hand value, if it lowered to a modeled form.
    pub value: Option<ArgExpr>,
    /// The source span of the whole assignment.
    pub span: TextSpan,
}

/// One normalized argument inside a call.
#[derive(Debug, Clone)]
pub enum ArgExpr {
    /// A string literal, with quotes stripped.
    Str(String),
    /// A dotted path reference (`libs.guava`, `libs.bundles.networking`).
    Path(String),
    /// A nested call (`project(":core")`).
    Call(CallExpr),
    /// A `name = value` / `name: value` named argument.
    Named {
        /// The argument name.
        name: String,
        /// The argument value.
        value: Box<ArgExpr>,
    },
}

impl ArgExpr {
    /// Returns the inner string if this arg is a [`ArgExpr::Str`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ArgExpr::Str(text) => Some(text),
            _ => None,
        }
    }
}

/// A Kotlin plugin infix suffix folded onto a plugin call.
#[derive(Debug, Clone)]
pub struct PluginSuffix {
    /// The suffix keyword (`version`, `apply`).
    pub keyword: String,
    /// The suffix value (`"1.0"`, `false`), if present.
    pub value: Option<String>,
}

/// Lowers the direct child statements of `node` into normalized [`Statement`]s for `lang`.
///
/// `node` is either the document root or a block/closure body. Unrecognized children
/// (including `OPAQUE`/`ERROR_NODE`) are skipped, so the result contains only nucleus
/// statements. This is the single entry the extraction driver uses to descend a tree.
pub fn child_statements(node: &SyntaxNode, lang: DslLanguage) -> Vec<Statement> {
    match lang {
        DslLanguage::Kotlin => kotlin::child_statements(node),
        DslLanguage::Groovy => groovy::child_statements(node),
    }
}

/// Strips matching surrounding quotes from a string literal token's text.
///
/// Handles `'`, `"`, and triple-quoted forms; returns the inner content. Input without
/// surrounding quotes is returned trimmed, so a malformed literal degrades gracefully.
pub(crate) fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    for quote in ["\"\"\"", "'''"] {
        if trimmed.len() >= 6 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return trimmed[3..trimmed.len() - 3].to_string();
        }
    }
    for quote in ['"', '\''] {
        if trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}
