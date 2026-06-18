//! Kotlin-DSL syntax kinds layered on the substrate's open [`SyntaxKind`] tag space.
//!
//! Every kind here is allocated at or above [`SyntaxKind::FIRST_CUSTOM`] so it never
//! collides with the substrate's built-in band (the built-ins are never renumbered). The
//! frontend wraps recognized nucleus constructs in these nodes; anything it does not model
//! stays in the substrate's generic [`SyntaxKind::OPAQUE`] node, so the two spaces compose
//! cleanly and a downstream consumer can tell "Kotlin nucleus" from "tolerated opaque run".

use crate::gradle::syntax::SyntaxKind;

/// Allocates a Kotlin kind at `FIRST_CUSTOM + offset`.
const fn kt(offset: u16) -> SyntaxKind {
    SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + offset)
}

/// An `import` statement (e.g. `import org.gradle.api.Project`).
pub const IMPORT: SyntaxKind = kt(0);
/// A `package` header (rare in build scripts, but legal).
pub const PACKAGE: SyntaxKind = kt(1);
/// A property assignment (`group = "..."`, `extra["x"] = "..."`).
pub const ASSIGNMENT: SyntaxKind = kt(2);
/// A call statement: an access path plus optional type args, an argument list, and/or a
/// trailing lambda block (`plugins { }`, `implementation("g:a:v")`, `tasks.register<T>("n") {}`).
pub const CALL: SyntaxKind = kt(3);
/// A dotted access path, possibly with index/call suffixes (`libs.bundles.x`, `tasks`).
pub const ACCESS_PATH: SyntaxKind = kt(4);
/// A parenthesized argument list (`(a, b = c)`).
pub const ARG_LIST: SyntaxKind = kt(5);
/// A named argument inside an argument list (`name = value`).
pub const NAMED_ARG: SyntaxKind = kt(6);
/// A type-argument list (`<Test>`, `<List<String>>`), kept structural (no generics semantics).
pub const TYPE_ARGS: SyntaxKind = kt(7);
/// A brace-delimited block / trailing lambda body (`{ ... }`).
pub const BLOCK: SyntaxKind = kt(8);
/// A bracket-delimited list literal (`[a, b]`) — uncommon in Kotlin DSL but tolerated.
pub const LIST: SyntaxKind = kt(9);
/// A bracket index suffix (`extra["x"]`).
pub const INDEX: SyntaxKind = kt(10);
/// An infix plugin suffix folded into a `CALL` (`version "1.0"`, `apply false`).
pub const PLUGIN_SUFFIX: SyntaxKind = kt(11);

/// Returns a short debug name for a Kotlin or built-in kind (used by the demo and dumps).
pub fn kind_name(kind: SyntaxKind) -> &'static str {
    match kind {
        IMPORT => "IMPORT",
        PACKAGE => "PACKAGE",
        ASSIGNMENT => "ASSIGNMENT",
        CALL => "CALL",
        ACCESS_PATH => "ACCESS_PATH",
        ARG_LIST => "ARG_LIST",
        NAMED_ARG => "NAMED_ARG",
        TYPE_ARGS => "TYPE_ARGS",
        BLOCK => "BLOCK",
        LIST => "LIST",
        INDEX => "INDEX",
        PLUGIN_SUFFIX => "PLUGIN_SUFFIX",
        other => other.builtin_name(),
    }
}

/// Keywords that structurally resemble a call/assignment but are OUTSIDE the nucleus.
///
/// These head a declaration or control-flow construct the frontend does not model, so the
/// top-level dispatch routes them straight to the brace-aware opaque consumer instead of
/// mis-parsing them as a generic call. No Kotlin control-flow/declaration semantics are
/// implied — this is purely a routing denylist.
pub const NON_NUCLEUS_KEYWORDS: &[&str] = &[
    "if", "when", "for", "while", "do", "fun", "class", "object", "interface", "val", "var",
    "return", "throw", "try", "while", "else",
];

/// Identifiers that commonly head a nucleus block, used as opaque-run STOP boundaries.
///
/// When the opaque consumer is back at brace depth 0 and sees one of these, it stops so the
/// following nucleus block (e.g. `dependencies { }`) is parsed as a real `CALL` rather than
/// swallowed into the opaque run.
pub const NUCLEUS_STARTERS: &[&str] = &[
    "import",
    "package",
    "plugins",
    "pluginManagement",
    "repositories",
    "dependencies",
    "dependencyResolutionManagement",
    "tasks",
    "group",
    "version",
    "extra",
    "rootProject",
    "include",
    "subprojects",
    "allprojects",
];

/// Returns `true` if `text` heads a non-nucleus construct (the routing denylist).
pub fn is_non_nucleus_keyword(text: &str) -> bool {
    NON_NUCLEUS_KEYWORDS.contains(&text)
}

/// Returns `true` if `text` is a known nucleus-block starter (an opaque STOP boundary).
pub fn is_nucleus_starter(text: &str) -> bool {
    NUCLEUS_STARTERS.contains(&text)
}
