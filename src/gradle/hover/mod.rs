//! Static hover (`textDocument/hover`) from LOCAL facts only.
//!
//! This is Task 13's hover half: a concise, localized summary of the construct under the
//! cursor, derived ENTIRELY from the static analysis tier — the semantic [`SemanticGraph`]
//! and the tolerant red tree. No web or documentation fetching is performed.
//!
//! Resolution priority at an offset:
//!
//! 1. The smallest semantic FACT whose source span contains the offset ([`facts`]):
//!    a dependency (string/project notation, or a `libs.*` accessor with its resolved
//!    coordinate), a task summary, or a plugin application.
//! 2. Otherwise, a known block-keyword call head (`plugins`/`dependencies`/`repositories`/
//!    `tasks`) at the offset ([`blocks`]) renders that block's purpose.
//! 3. Otherwise `None` — an opaque, unknown, or unsupported position yields no hover.
//!
//! # LSP-type-free
//!
//! The public entry [`hover`] returns an internal [`HoverModel`] carrying a [`MessageKey`] +
//! args (rendered at the server boundary) and the source [`TextSpan`] to highlight. The
//! server converts the message to text and the span to a `Range`. Malformed input degrades to
//! `None` and never panics.
//!
//! # Task-16 enrichment seam
//!
//! Today every [`HoverModel`] is built from STATIC facts. The advanced (sidecar-backed) tier
//! enriches hover WITHOUT changing this layer: Task 16 composes its plugin/classpath-derived
//! hover AFTER this static result (e.g. preferring a sidecar plugin description for a
//! [`MessageKey::HoverPlugin`] result, or adding hover for a plugin-contributed member this
//! static layer returns `None` for). The static entry's signature and contract stay fixed;
//! the seam is "call [`hover`] first, then layer enrichment on its `Option<HoverModel>`".
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::hover::hover;
//! use gradle_analyzer::gradle::parser::parse_groovy;
//! use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput};
//! use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
//! use gradle_analyzer::i18n::MessageKey;
//! use tower_lsp::lsp_types::Url;
//!
//! let text = "task build {}\n";
//! let uri = Url::from_file_path("/proj/build.gradle").unwrap();
//! let kind = GradleFileKind::RootBuildScript(DslLanguage::Groovy);
//! let doc = TrackedDocument::new(uri, 1, text, kind);
//! let parse = parse_groovy(text);
//! let input = SemanticInput::script("build.gradle", text, kind);
//! let graph = analyze_documents(std::slice::from_ref(&input));
//!
//! // Hover inside `task build {}` summarizes the task.
//! let offset = text.find("build").unwrap() + 1;
//! let model = hover(&doc, &parse, &graph, offset).unwrap();
//! assert_eq!(model.message_key, MessageKey::HoverTask);
//! ```

pub mod blocks;
pub mod facts;

#[cfg(test)]
mod tests;

use crate::gradle::semantic::SemanticGraph;
use crate::gradle::syntax::{Parse, SyntaxNode, TextSpan};
use crate::gradle::workspace::TrackedDocument;
use crate::i18n::MessageKey;

use super::code_actions::document_id_for;

/// A localizable hover result: the message to render and the source span it describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverModel {
    /// The message key for the hover content.
    pub message_key: MessageKey,
    /// Positional arguments substituted into the message template.
    pub args: Vec<String>,
    /// The source byte span the hover describes (the server renders it as the hover range).
    pub span: TextSpan,
}

impl HoverModel {
    /// Builds a hover model from a key, args, and the described span.
    pub fn new(message_key: MessageKey, args: Vec<String>, span: TextSpan) -> HoverModel {
        HoverModel {
            message_key,
            args,
            span,
        }
    }
}

/// Computes static hover content at byte `offset` in `doc`, if any.
///
/// `parse` must be the [`Parse`] of `doc`'s current text; `graph` supplies the semantic facts.
/// Returns `None` for a no-DSL document, an opaque/unknown position, or any position with no
/// confident local fact or block keyword. The result is LSP-type-free; the server boundary
/// renders the message and converts the span to a `Range`.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::hover::hover;
/// use gradle_analyzer::gradle::parser::parse_kotlin;
/// use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput};
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
/// use tower_lsp::lsp_types::Url;
///
/// let text = "plugins {\n    id(\"java\")\n}\n";
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
/// let kind = GradleFileKind::RootBuildScript(DslLanguage::Kotlin);
/// let doc = TrackedDocument::new(uri, 1, text, kind);
/// let parse = parse_kotlin(text);
/// let input = SemanticInput::script("build.gradle.kts", text, kind);
/// let graph = analyze_documents(std::slice::from_ref(&input));
///
/// // Hover on the `plugins` keyword explains the block.
/// assert!(hover(&doc, &parse, &graph, 2).is_some());
/// ```
pub fn hover(
    doc: &TrackedDocument,
    parse: &Parse,
    graph: &SemanticGraph,
    offset: usize,
) -> Option<HoverModel> {
    let span = tracing::trace_span!("hover.build", uri = %doc.uri(), offset);
    let _enter = span.enter();

    let language = doc.kind().dsl()?;

    let document = document_id_for(doc);
    if let Some(semantics) = graph.document(&document)
        && let Some(model) = facts::hover_fact(semantics, offset)
    {
        tracing::trace!(key = %model.message_key, "hover resolved from fact");
        return Some(model);
    }

    let root = SyntaxNode::new_root(parse.green.clone());
    let model = blocks::hover_block_keyword(&root, language, offset);
    if let Some(model) = &model {
        tracing::trace!(key = %model.message_key, "hover resolved from block keyword");
    } else {
        tracing::trace!("no hover at offset");
    }
    model
}
