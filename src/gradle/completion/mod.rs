//! Static-first, context-aware completion engine for both Gradle DSLs.
//!
//! This is Task 11: a deterministic completion engine that answers
//! `textDocument/completion` from STATIC inputs only (the tolerant red tree + the
//! [`SemanticGraph`]) — never the sidecar. It is split into FOUR deterministic layers so
//! eligibility stays cleanly separate from ranking:
//!
//! 1. **Text-state suppression** ([`context`]) — no completion inside comments, string
//!    literals, or a non-block `OPAQUE`/`ERROR_NODE` region; such positions yield EMPTY.
//! 2. **Context classification** ([`context`]) — the red tree + a line-prefix scan lower the
//!    cursor to a [`context::CompletionContext`] (which block, and the position within it).
//! 3. **Candidate eligibility** ([`candidates`] + [`scope`]) — per-context builders emit a
//!    [`Candidate`] list in INSERTION order from static tables and [`SemanticGraph`] facts.
//! 4. **Ranking** ([`ranking`]) — a SEPARATE pass assigns a stable group rank by
//!    [`CandidateKind`], sorts `(rank, label)`, and caps to `max_candidates`.
//!
//! The public entry [`complete`] is LSP-type-free: it returns internal [`Candidate`]s that
//! the server boundary converts to `tower_lsp::lsp_types::CompletionItem`. All user-facing
//! `detail` text is rendered through a [`Translator`]/[`MessageKey`]; labels are source
//! identifiers and are intentionally NOT translated.
//!
//! # Task-16 enrichment seam
//!
//! The advanced (sidecar-backed) tier adds plugin-CONTRIBUTED candidates without changing
//! this layer's classification or ranking: [`candidates::collect_eligible`] runs BEFORE
//! [`ranking::rank`], so Task 16 appends [`CandidateKind::PluginContributed`] items to the
//! eligible vec and the existing ranking pass orders them uniformly. No static-tier code
//! promises plugin-contributed members today.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::completion::{complete, CompletionServices};
//! use gradle_analyzer::gradle::parser::parse_groovy;
//! use gradle_analyzer::gradle::semantic::analyze_documents;
//! use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
//! use gradle_analyzer::i18n::Translator;
//! use tower_lsp::lsp_types::Url;
//!
//! let text = "dependencies {\n    \n}\n";
//! let uri = Url::from_file_path("/proj/build.gradle").unwrap();
//! let doc = TrackedDocument::new(uri, 1, text, GradleFileKind::RootBuildScript(DslLanguage::Groovy));
//! let parse = parse_groovy(text);
//! let graph = analyze_documents(&[]);
//! let translator = Translator::new();
//! let services = CompletionServices::new(&translator, 50);
//!
//! // Offset on the blank line inside `dependencies { }` offers configurations + a scaffold.
//! let offset = text.find("\n    \n").unwrap() + 5;
//! let candidates = complete(&doc, &parse, &graph, offset, &services);
//! assert!(candidates.iter().any(|c| c.label == "implementation"));
//! ```

pub mod candidates;
pub mod context;
pub mod ranking;
pub mod scope;

use std::path::Path;

use tracing::trace;

use crate::gradle::semantic::{SemanticGraph, SemanticInput};
use crate::gradle::syntax::{Parse, SyntaxNode};
use crate::gradle::workspace::{
    DslLanguage, GradleFileKind, TrackedDocument, detect_workspace_root,
};
use crate::i18n::Translator;

use scope::VisibleScope;

/// The classification of a completion [`Candidate`], independent of its label text.
///
/// The variant order is also the group-rank order used by [`ranking::rank`], so a more
/// context-specific kind (e.g. a catalog accessor inside `dependencies {`) sorts ahead of a
/// generic keyword. [`CandidateKind::PluginContributed`] is the Task-16 enrichment seam: it
/// is never produced by the static tier, but is ranked uniformly when Task 16 appends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    /// A top-level block keyword (`plugins`, `dependencies`, ...).
    BlockKeyword,
    /// A dependency configuration (`implementation`, `api`, ...).
    DependencyConfiguration,
    /// A version-catalog accessor (`libs.guava`, `libs.bundles.networking`, ...).
    CatalogAccessor,
    /// A coordinate scaffold template for string-notation dependencies.
    CoordinateScaffold,
    /// A statically-known plugin id (`java`, `org.jetbrains.kotlin.jvm`, ...).
    PluginId,
    /// A repository function (`mavenCentral`, `google`, ...).
    Repository,
    /// A task name from the semantic graph.
    TaskName,
    /// A project path (`:app`, `:core`) from the semantic graph.
    ProjectPath,
    /// A safe `import` hint.
    ImportHint,
    /// RESERVED for Task 16: a plugin-contributed member. Never produced statically.
    PluginContributed,
}

/// One completion candidate: a source-identifier label plus localized detail.
///
/// `label` is the exact text a user would type (a source identifier — never translated).
/// `detail` is user-facing explanatory text rendered through a [`Translator`]. `insert_text`
/// overrides the inserted text when it must differ from the label (e.g. a catalog accessor
/// completed after `libs.` inserts only the remaining suffix, and a coordinate scaffold
/// inserts a templated string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The completion label (a source identifier; not translated).
    pub label: String,
    /// The candidate's classification (drives ranking and the LSP item kind).
    pub kind: CandidateKind,
    /// User-facing detail text, rendered via [`Translator`]/[`MessageKey`].
    pub detail: String,
    /// Text to insert if it must differ from `label` (otherwise the label is inserted).
    pub insert_text: Option<String>,
}

impl Candidate {
    /// Builds a candidate with `label` inserted verbatim.
    pub fn new(label: impl Into<String>, kind: CandidateKind, detail: impl Into<String>) -> Candidate {
        Candidate {
            label: label.into(),
            kind,
            detail: detail.into(),
            insert_text: None,
        }
    }

    /// Builds a candidate that inserts `insert_text` instead of its label.
    pub fn with_insert(
        label: impl Into<String>,
        kind: CandidateKind,
        detail: impl Into<String>,
        insert_text: impl Into<String>,
    ) -> Candidate {
        Candidate {
            label: label.into(),
            kind,
            detail: detail.into(),
            insert_text: Some(insert_text.into()),
        }
    }
}

/// Shared services the engine reads at the boundary: the translator and the candidate cap.
///
/// Constructed at the server boundary from the [`Translator`] and the
/// `completion.max_candidates` knob read from [`crate::config::ConfigManager`], so neither
/// the cap nor any user-facing string is hardcoded inside the engine.
#[derive(Debug, Clone, Copy)]
pub struct CompletionServices<'a> {
    /// Renders candidate `detail` text from typed message keys.
    pub translator: &'a Translator,
    /// The maximum number of candidates returned (post-ranking cap).
    pub max_candidates: usize,
}

impl<'a> CompletionServices<'a> {
    /// Builds services from a translator and a positive candidate cap.
    pub fn new(translator: &'a Translator, max_candidates: usize) -> CompletionServices<'a> {
        CompletionServices {
            translator,
            max_candidates,
        }
    }
}

/// Computes static, context-aware completion candidates at `offset` in `doc`.
///
/// The pipeline is: classify (text-state suppression + AST/line context) → collect eligible
/// candidates (static tables + [`SemanticGraph`] facts) → rank + cap. A suppressed position
/// (comment / string / non-block opaque region) yields an EMPTY vec. Malformed or unclosed
/// input degrades to sensible-or-empty and never panics. The result is LSP-type-free; the
/// server boundary converts each [`Candidate`] to a `CompletionItem`.
///
/// `parse` must be the [`Parse`] of `doc`'s current text; `graph` supplies task names,
/// project paths, buildSrc symbols, and version-catalog accessors.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::completion::{complete, CompletionServices};
/// use gradle_analyzer::gradle::parser::parse_kotlin;
/// use gradle_analyzer::gradle::semantic::analyze_documents;
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
/// use gradle_analyzer::i18n::Translator;
/// use tower_lsp::lsp_types::Url;
///
/// let text = "plugins {\n    \n}\n";
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
/// let doc = TrackedDocument::new(uri, 1, text, GradleFileKind::RootBuildScript(DslLanguage::Kotlin));
/// let parse = parse_kotlin(text);
/// let graph = analyze_documents(&[]);
/// let tr = Translator::new();
/// let services = CompletionServices::new(&tr, 50);
///
/// let offset = text.find("\n    \n").unwrap() + 5;
/// let items = complete(&doc, &parse, &graph, offset, &services);
/// assert!(items.iter().any(|c| c.label == "id")); // plugin-id helper inside `plugins {`
/// ```
pub fn complete(
    doc: &TrackedDocument,
    parse: &Parse,
    graph: &SemanticGraph,
    offset: usize,
    services: &CompletionServices,
) -> Vec<Candidate> {
    let lang = doc.kind().dsl().unwrap_or(DslLanguage::Groovy);
    let text = doc.text();
    let root = SyntaxNode::new_root(parse.green.clone());

    let classify_span = tracing::trace_span!("completion.classify", offset, ?lang);
    let context = {
        let _enter = classify_span.enter();
        context::classify(&root, text, offset, lang)
    };

    let Some(context) = context else {
        trace!("completion suppressed (comment/string/opaque)");
        return Vec::new();
    };

    let build_span = tracing::trace_span!("completion.build", block = ?context.block);
    let _enter = build_span.enter();
    let scope = VisibleScope::gather(graph);
    let eligible = candidates::collect_eligible(&context, &scope, services);
    let ranked = ranking::rank(eligible, services.max_candidates);
    trace!(candidates = ranked.len(), "completion built");
    ranked
}

/// Builds the semantic-analysis inputs for `doc`, plus any on-disk version catalog.
///
/// Static-first: this reads the workspace's `*.versions.toml` catalog from disk (so
/// `libs.*` accessors resolve) but never launches the sidecar. When no workspace root or
/// catalog is found, the result is just `doc` itself. Used by the server boundary to build
/// the [`SemanticGraph`] passed to [`complete`].
pub fn workspace_inputs(doc: &TrackedDocument) -> Vec<SemanticInput> {
    let mut inputs = Vec::new();
    if let Some(root) = doc.uri().to_file_path().ok().and_then(|p| detect_workspace_root(&p)) {
        collect_catalog_inputs(root.path(), &mut inputs);
    }
    inputs.push(doc_input(doc));
    inputs
}

/// Reads known catalog file locations under `root` into [`SemanticInput`]s.
fn collect_catalog_inputs(root: &Path, inputs: &mut Vec<SemanticInput>) {
    for relative in ["gradle/libs.versions.toml", "libs.versions.toml"] {
        let path = root.join(relative);
        if let Ok(text) = std::fs::read_to_string(&path) {
            inputs.push(SemanticInput::script(relative, text, GradleFileKind::VersionCatalog));
        }
    }
}

/// Lowers a tracked document into a semantic input keyed by its file name.
fn doc_input(doc: &TrackedDocument) -> SemanticInput {
    let id = doc
        .uri()
        .to_file_path()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| doc.uri().as_str().to_string());
    SemanticInput::script(&id, doc.text().to_string(), doc.kind())
}

/// Converts an LSP line/character position into a byte offset into `text`.
///
/// `character` is interpreted as UTF-16 code units (the LSP default), so multi-byte and
/// astral-plane characters map correctly. A position past the end of a line clamps to the
/// line end; a line past the end of the text clamps to the text length, so the result is
/// always a valid byte boundary and the call never panics.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::completion::byte_offset_at;
///
/// let text = "ab\ncd\n";
/// assert_eq!(byte_offset_at(text, 0, 1), 1); // 'b'
/// assert_eq!(byte_offset_at(text, 1, 1), 4); // 'd'
/// assert_eq!(byte_offset_at(text, 9, 9), text.len()); // clamps past end
/// ```
pub fn byte_offset_at(text: &str, line: u32, character: u32) -> usize {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (idx, ch) in text.char_indices() {
        if current_line == line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = idx + ch.len_utf8();
        }
    }
    if current_line < line {
        return text.len();
    }

    let line_text = &text[line_start..];
    let mut utf16_units = 0u32;
    for (idx, ch) in line_text.char_indices() {
        if ch == '\n' || utf16_units >= character {
            return line_start + idx;
        }
        utf16_units += ch.len_utf16() as u32;
    }
    text.len()
}

#[cfg(test)]
mod tests;
