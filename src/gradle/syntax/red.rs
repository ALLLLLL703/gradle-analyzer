//! The red tree: a navigable cursor layered over the green tree.
//!
//! The green layer ([`super::green`]) is compact and parent-free; the red layer adds the
//! two things a consumer needs to navigate: an absolute byte `offset` for every node/token
//! and a parent pointer. Red nodes are built eagerly here (Gradle build files are small, so
//! the simplicity outweighs lazy-cursor savings) using [`std::rc::Rc::new_cyclic`] to install
//! a [`Weak`] parent link without unsafe code. Red nodes borrow nothing from the source, so
//! [`SyntaxNode::text`] reconstructs the exact bytes from the owned green tokens.

use std::rc::{Rc, Weak};
use std::sync::Arc;

use super::green::{GreenChild, GreenNode, GreenToken};
use super::span::TextSpan;
use super::token::SyntaxKind;

/// A node in the red tree: a green node plus its absolute offset and parent link.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{GreenChild, GreenNode, GreenToken, SyntaxKind, SyntaxNode};
///
/// let green = GreenNode::new(
///     SyntaxKind::ROOT,
///     vec![
///         GreenChild::token(GreenToken::new(SyntaxKind::IDENT, "foo")),
///         GreenChild::token(GreenToken::new(SyntaxKind::WHITESPACE, " ")),
///         GreenChild::token(GreenToken::new(SyntaxKind::IDENT, "bar")),
///     ],
/// );
/// let root = SyntaxNode::new_root(green);
/// assert_eq!(root.text(), "foo bar");
/// assert_eq!(root.span().start, 0);
/// assert_eq!(root.children().len(), 3);
/// ```
#[derive(Debug, Clone)]
pub struct SyntaxNode {
    green: Arc<GreenNode>,
    offset: usize,
    parent: Option<Weak<SyntaxNode>>,
    children: Vec<SyntaxElement>,
}

/// A leaf in the red tree: a green token plus its absolute offset and parent link.
#[derive(Debug, Clone)]
pub struct SyntaxToken {
    green: Arc<GreenToken>,
    offset: usize,
    parent: Weak<SyntaxNode>,
}

/// Either a red node or a red token, the uniform child type for navigation.
#[derive(Debug, Clone)]
pub enum SyntaxElement {
    /// A nested red node.
    Node(Rc<SyntaxNode>),
    /// A leaf red token.
    Token(Rc<SyntaxToken>),
}

impl SyntaxNode {
    /// Builds the red root from an owned green node, computing every descendant offset.
    pub fn new_root(green: GreenNode) -> Rc<SyntaxNode> {
        Self::build(Arc::new(green), 0, None)
    }

    fn build(green: Arc<GreenNode>, offset: usize, parent: Option<Weak<SyntaxNode>>) -> Rc<SyntaxNode> {
        Rc::new_cyclic(|me: &Weak<SyntaxNode>| {
            let mut children = Vec::with_capacity(green.children().len());
            let mut cursor = offset;
            for child in green.children() {
                match child {
                    GreenChild::Node(node) => {
                        let red = SyntaxNode::build(Arc::clone(node), cursor, Some(me.clone()));
                        cursor += node.width();
                        children.push(SyntaxElement::Node(red));
                    }
                    GreenChild::Token(token) => {
                        let red = Rc::new(SyntaxToken {
                            green: Arc::clone(token),
                            offset: cursor,
                            parent: me.clone(),
                        });
                        cursor += token.width();
                        children.push(SyntaxElement::Token(red));
                    }
                }
            }
            SyntaxNode { green, offset, parent, children }
        })
    }

    /// Returns this node's syntax kind.
    pub fn kind(&self) -> SyntaxKind {
        self.green.kind()
    }

    /// Returns the absolute byte span this node covers.
    pub fn span(&self) -> TextSpan {
        TextSpan::new(self.offset, self.green.width())
    }

    /// Returns the parent node, or `None` at the root.
    pub fn parent(&self) -> Option<Rc<SyntaxNode>> {
        self.parent.as_ref().and_then(Weak::upgrade)
    }

    /// Returns this node's direct children (nodes and tokens) in order.
    pub fn children(&self) -> &[SyntaxElement] {
        &self.children
    }

    /// Returns only the child nodes, skipping tokens.
    pub fn child_nodes(&self) -> impl Iterator<Item = Rc<SyntaxNode>> + '_ {
        self.children.iter().filter_map(|child| match child {
            SyntaxElement::Node(node) => Some(Rc::clone(node)),
            SyntaxElement::Token(_) => None,
        })
    }

    /// Reconstructs the exact source text covered by this node.
    pub fn text(&self) -> String {
        self.green.text()
    }
}

impl SyntaxToken {
    /// Returns this token's syntax kind.
    pub fn kind(&self) -> SyntaxKind {
        self.green.kind()
    }

    /// Returns the absolute byte span this token covers.
    pub fn span(&self) -> TextSpan {
        TextSpan::new(self.offset, self.green.width())
    }

    /// Returns the token's owned source text.
    pub fn text(&self) -> &str {
        self.green.text()
    }

    /// Returns the parent node, or `None` if it has been dropped.
    pub fn parent(&self) -> Option<Rc<SyntaxNode>> {
        self.parent.upgrade()
    }
}

impl SyntaxElement {
    /// Returns the element's syntax kind.
    pub fn kind(&self) -> SyntaxKind {
        match self {
            SyntaxElement::Node(node) => node.kind(),
            SyntaxElement::Token(token) => token.kind(),
        }
    }

    /// Returns the element's absolute byte span.
    pub fn span(&self) -> TextSpan {
        match self {
            SyntaxElement::Node(node) => node.span(),
            SyntaxElement::Token(token) => token.span(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(kind: SyntaxKind, text: &str) -> GreenChild {
        GreenChild::token(GreenToken::new(kind, text))
    }

    fn sample() -> Rc<SyntaxNode> {
        // ROOT[ "outer" WS BLOCK[ "{" "inner" "}" ] ]
        let block = GreenNode::new(
            SyntaxKind::OPAQUE,
            vec![
                token(SyntaxKind::PUNCT, "{"),
                token(SyntaxKind::IDENT, "inner"),
                token(SyntaxKind::PUNCT, "}"),
            ],
        );
        let green = GreenNode::new(
            SyntaxKind::ROOT,
            vec![
                token(SyntaxKind::IDENT, "outer"),
                token(SyntaxKind::WHITESPACE, " "),
                GreenChild::node(block),
            ],
        );
        SyntaxNode::new_root(green)
    }

    #[test]
    fn offsets_are_absolute_and_text_roundtrips() {
        let root = sample();
        assert_eq!(root.text(), "outer {inner}");
        assert_eq!(root.span(), TextSpan::new(0, 13));

        let block = root.child_nodes().next().expect("one child node");
        assert_eq!(block.span(), TextSpan::new(6, 7));
        assert_eq!(block.text(), "{inner}");
    }

    #[test]
    fn parent_links_upgrade_back_to_root() {
        let root = sample();
        let block = root.child_nodes().next().unwrap();
        let parent = block.parent().expect("block has a parent");
        assert_eq!(parent.kind(), SyntaxKind::ROOT);
        assert!(root.parent().is_none());
    }

    #[test]
    fn token_spans_track_running_offset() {
        let root = sample();
        let spans: Vec<_> = root
            .children()
            .iter()
            .map(SyntaxElement::span)
            .collect();
        assert_eq!(spans[0], TextSpan::new(0, 5));
        assert_eq!(spans[1], TextSpan::new(5, 1));
        assert_eq!(spans[2], TextSpan::new(6, 7));
    }
}
