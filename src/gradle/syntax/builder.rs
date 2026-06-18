//! A checkpoint-based builder that assembles a [`GreenNode`] tree.
//!
//! The builder is the rowan-style construction API the parser drives: it keeps a stack of
//! in-progress nodes and a flat list of completed children, so [`GreenNodeBuilder::token`]
//! and [`GreenNodeBuilder::start_node`]/[`GreenNodeBuilder::finish_node`] build the tree
//! bottom-up in source order. [`GreenNodeBuilder::checkpoint`] records a position that a
//! later [`GreenNodeBuilder::start_node_at`] can retroactively wrap — the trick that lets a
//! recursive-descent parser build left-associative structure without left recursion.

use super::green::{GreenChild, GreenNode, GreenToken};
use super::token::SyntaxKind;

/// A position in the child stream that a later node can retroactively wrap.
///
/// Obtained from [`GreenNodeBuilder::checkpoint`] and consumed by
/// [`GreenNodeBuilder::start_node_at`]. It is just an index into the current parent's
/// pending children, so it is cheap and `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint(usize);

struct InProgress {
    kind: SyntaxKind,
    children: Vec<GreenChild>,
}

/// Builds an immutable [`GreenNode`] tree via a node/token/checkpoint API.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{GreenNodeBuilder, SyntaxKind};
///
/// let mut builder = GreenNodeBuilder::new();
/// builder.start_node(SyntaxKind::ROOT);
/// let cp = builder.checkpoint();
/// builder.token(SyntaxKind::IDENT, "a");
/// // Retroactively wrap the `a` token in an OPAQUE node:
/// builder.start_node_at(cp, SyntaxKind::OPAQUE);
/// builder.finish_node();
/// builder.finish_node();
///
/// let root = builder.finish();
/// assert_eq!(root.text(), "a");
/// assert_eq!(root.children().len(), 1); // the OPAQUE wrapper
/// ```
pub struct GreenNodeBuilder {
    stack: Vec<InProgress>,
}

impl GreenNodeBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Opens a new node of `kind`; subsequent tokens/nodes become its children.
    pub fn start_node(&mut self, kind: SyntaxKind) {
        self.stack.push(InProgress { kind, children: Vec::new() });
    }

    /// Records a checkpoint at the current end of the open node's children.
    ///
    /// Calling [`GreenNodeBuilder::checkpoint`] before any node is open records position 0
    /// of an implicit root, which is still a valid `start_node_at` target.
    pub fn checkpoint(&mut self) -> Checkpoint {
        let len = self.stack.last().map_or(0, |frame| frame.children.len());
        Checkpoint(len)
    }

    /// Retroactively opens a node of `kind` that adopts every child added since `checkpoint`.
    ///
    /// The children recorded after the checkpoint are moved into the new node, which then
    /// becomes the open node — so a following [`GreenNodeBuilder::finish_node`] closes it.
    pub fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        let frame = self.stack.last_mut().expect("start_node_at needs an open node");
        let at = checkpoint.0.min(frame.children.len());
        let adopted = frame.children.split_off(at);
        self.stack.push(InProgress { kind, children: adopted });
    }

    /// Adds a leaf token of `kind` with the exact `text` it covers.
    pub fn token(&mut self, kind: SyntaxKind, text: impl Into<String>) {
        let token = GreenChild::token(GreenToken::new(kind, text));
        self.push_child(token);
    }

    /// Closes the current node and attaches it to its parent (or stages it as the result).
    pub fn finish_node(&mut self) {
        let done = self.stack.pop().expect("finish_node without a matching start_node");
        let node = GreenChild::node(GreenNode::new(done.kind, done.children));
        if self.stack.is_empty() {
            self.stack.push(InProgress { kind: SyntaxKind::ROOT, children: vec![node] });
        } else {
            self.push_child(node);
        }
    }

    /// Finishes the build and returns the single root [`GreenNode`].
    ///
    /// Panics if the node stack is not balanced (every `start_node` matched by a
    /// `finish_node`), which would indicate a builder-usage bug rather than bad input.
    pub fn finish(mut self) -> GreenNode {
        assert_eq!(
            self.stack.len(),
            1,
            "unbalanced builder: every start_node needs a finish_node"
        );
        let mut root = self.stack.pop().expect("one frame remains");
        if root.children.len() == 1
            && let Some(GreenChild::Node(node)) = root.children.first()
        {
            return (**node).clone();
        }
        GreenNode::new(SyntaxKind::ROOT, std::mem::take(&mut root.children))
    }

    fn push_child(&mut self, child: GreenChild) {
        match self.stack.last_mut() {
            Some(frame) => frame.children.push(child),
            None => self.stack.push(InProgress {
                kind: SyntaxKind::ROOT,
                children: vec![child],
            }),
        }
    }
}

impl Default for GreenNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_token_sequence_roundtrips() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ROOT);
        builder.token(SyntaxKind::IDENT, "foo");
        builder.token(SyntaxKind::WHITESPACE, " ");
        builder.token(SyntaxKind::NUMBER, "42");
        builder.finish_node();

        let root = builder.finish();
        assert_eq!(root.text(), "foo 42");
        assert_eq!(root.children().len(), 3);
    }

    #[test]
    fn nested_nodes_track_width() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ROOT);
        builder.token(SyntaxKind::IDENT, "block");
        builder.start_node(SyntaxKind::OPAQUE);
        builder.token(SyntaxKind::PUNCT, "{");
        builder.token(SyntaxKind::PUNCT, "}");
        builder.finish_node();
        builder.finish_node();

        let root = builder.finish();
        assert_eq!(root.text(), "block{}");
        assert_eq!(root.width(), 7);
        assert_eq!(root.children().len(), 2);
    }

    #[test]
    fn start_node_at_retroactively_wraps_children() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ROOT);
        let cp = builder.checkpoint();
        builder.token(SyntaxKind::IDENT, "a");
        builder.token(SyntaxKind::PUNCT, "+");
        builder.token(SyntaxKind::IDENT, "b");
        builder.start_node_at(cp, SyntaxKind::OPAQUE);
        builder.finish_node();
        builder.finish_node();

        let root = builder.finish();
        assert_eq!(root.text(), "a+b");
        assert_eq!(root.children().len(), 1);
        let wrapper = root.children().first().unwrap();
        assert_eq!(wrapper.kind(), SyntaxKind::OPAQUE);
        assert_eq!(wrapper.width(), 3);
    }

    #[test]
    fn checkpoint_only_wraps_children_after_it() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::ROOT);
        builder.token(SyntaxKind::IDENT, "keep");
        let cp = builder.checkpoint();
        builder.token(SyntaxKind::IDENT, "wrap");
        builder.start_node_at(cp, SyntaxKind::OPAQUE);
        builder.finish_node();
        builder.finish_node();

        let root = builder.finish();
        assert_eq!(root.children().len(), 2);
        assert_eq!(root.children()[0].kind(), SyntaxKind::IDENT);
        assert_eq!(root.children()[1].kind(), SyntaxKind::OPAQUE);
    }
}
