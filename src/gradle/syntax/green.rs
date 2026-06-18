//! The immutable, shareable green tree.
//!
//! Green nodes are the storage layer: each [`GreenNode`] owns its children and caches the
//! total byte `width` of its subtree, and each [`GreenToken`] owns its text. Because tokens
//! own their text, a green tree reconstructs the exact original source by concatenation
//! ([`GreenNode::text`]) WITHOUT borrowing the input — which is what makes it safe to cache
//! and share (`Arc`) across edits. Green nodes carry NO absolute offsets and NO parent
//! pointers; those live in the red layer ([`super::red`]).

use std::sync::Arc;

use super::token::SyntaxKind;

/// A child of a [`GreenNode`]: either a nested node or a leaf token.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{GreenChild, GreenToken, SyntaxKind};
///
/// let child = GreenChild::token(GreenToken::new(SyntaxKind::IDENT, "foo"));
/// assert_eq!(child.width(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreenChild {
    /// A nested node.
    Node(Arc<GreenNode>),
    /// A leaf token.
    Token(Arc<GreenToken>),
}

impl GreenChild {
    /// Wraps a node as a child.
    pub fn node(node: GreenNode) -> Self {
        GreenChild::Node(Arc::new(node))
    }

    /// Wraps a token as a child.
    pub fn token(token: GreenToken) -> Self {
        GreenChild::Token(Arc::new(token))
    }

    /// Returns the byte width of this child's subtree.
    pub fn width(&self) -> usize {
        match self {
            GreenChild::Node(node) => node.width(),
            GreenChild::Token(token) => token.width(),
        }
    }

    /// Returns this child's syntax kind.
    pub fn kind(&self) -> SyntaxKind {
        match self {
            GreenChild::Node(node) => node.kind(),
            GreenChild::Token(token) => token.kind(),
        }
    }

    /// Appends this child's source text to `out`.
    pub fn write_text(&self, out: &mut String) {
        match self {
            GreenChild::Node(node) => node.write_text(out),
            GreenChild::Token(token) => out.push_str(token.text()),
        }
    }
}

/// An immutable leaf token owning its source text.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{GreenToken, SyntaxKind};
///
/// let token = GreenToken::new(SyntaxKind::STRING, "\"hi\"");
/// assert_eq!(token.width(), 4);
/// assert_eq!(token.text(), "\"hi\"");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreenToken {
    kind: SyntaxKind,
    text: String,
}

impl GreenToken {
    /// Builds a token from a kind and the exact source text it covers.
    pub fn new(kind: SyntaxKind, text: impl Into<String>) -> Self {
        Self { kind, text: text.into() }
    }

    /// Returns the token's kind.
    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }

    /// Returns the token's owned source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the byte width of the token text.
    pub fn width(&self) -> usize {
        self.text.len()
    }
}

/// An immutable interior node owning its children and caching its subtree width.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{GreenChild, GreenNode, GreenToken, SyntaxKind};
///
/// let node = GreenNode::new(
///     SyntaxKind::ROOT,
///     vec![
///         GreenChild::token(GreenToken::new(SyntaxKind::IDENT, "a")),
///         GreenChild::token(GreenToken::new(SyntaxKind::WHITESPACE, " ")),
///         GreenChild::token(GreenToken::new(SyntaxKind::IDENT, "b")),
///     ],
/// );
/// assert_eq!(node.width(), 3);
/// assert_eq!(node.text(), "a b");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreenNode {
    kind: SyntaxKind,
    width: usize,
    children: Vec<GreenChild>,
}

impl GreenNode {
    /// Builds a node from a kind and its children, caching the total width.
    pub fn new(kind: SyntaxKind, children: Vec<GreenChild>) -> Self {
        let width = children.iter().map(GreenChild::width).sum();
        Self { kind, width, children }
    }

    /// Returns the node's kind.
    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }

    /// Returns the cached byte width of the whole subtree.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the node's children in source order.
    pub fn children(&self) -> &[GreenChild] {
        &self.children
    }

    /// Reconstructs the exact source text of this subtree.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.width);
        self.write_text(&mut out);
        out
    }

    /// Appends this subtree's source text to `out`.
    pub fn write_text(&self, out: &mut String) {
        for child in &self.children {
            child.write_text(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(text: &str) -> GreenChild {
        GreenChild::token(GreenToken::new(SyntaxKind::IDENT, text))
    }

    #[test]
    fn width_is_sum_of_children() {
        let node = GreenNode::new(
            SyntaxKind::ROOT,
            vec![ident("foo"), ident("bar"), ident("baz")],
        );
        assert_eq!(node.width(), 9);
    }

    #[test]
    fn nested_text_roundtrips_by_concatenation() {
        let inner = GreenNode::new(SyntaxKind::OPAQUE, vec![ident("xy")]);
        let node = GreenNode::new(
            SyntaxKind::ROOT,
            vec![
                ident("a"),
                GreenChild::node(inner),
                GreenChild::token(GreenToken::new(SyntaxKind::PUNCT, "!")),
            ],
        );
        assert_eq!(node.width(), 4);
        assert_eq!(node.text(), "axy!");
    }

    #[test]
    fn empty_node_has_zero_width_and_text() {
        let node = GreenNode::new(SyntaxKind::ROOT, vec![]);
        assert_eq!(node.width(), 0);
        assert_eq!(node.text(), "");
    }
}
