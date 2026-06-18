//! The tolerant red-tree occurrence scanner: positions → navigable [`Occurrence`]s.
//!
//! Walks a parsed red tree and records every *navigable* token occurrence — a task name, a
//! `libs.*` catalog accessor, or a project path — with its **precise** source span and a
//! [`Symbol`]/[`OccurrenceRole`] classification. This is the precision layer the semantic
//! graph cannot provide (`dependsOn` is not a fact; fact spans are whole-call spans), while
//! the graph stays the source of truth for *definitions* (see [`super::definition`]).
//!
//! Scanning is DSL-aware via the two frontends' public `SyntaxKind` constants but stays
//! tolerant: it only recognizes call shapes, so `OPAQUE`/`ERROR_NODE` subtrees contribute
//! nothing, and malformed input simply yields fewer occurrences (never a panic).

use crate::gradle::parser::{groovy as gv, kotlin::kinds as kt};
use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, TextSpan};
use crate::gradle::workspace::DslLanguage;

/// A navigable symbol a position can resolve to or from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Symbol {
    /// A task addressed by name (`build`).
    Task(String),
    /// A version-catalog accessor as the dotted remainder after `libs.` (`guava`,
    /// `bundles.networking`, `plugins.spotless`).
    CatalogAccessor(String),
    /// A project addressed by Gradle path (`:app`).
    Project(String),
}

/// Whether an occurrence declares a symbol or references it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccurrenceRole {
    /// A declaration site (`task build`, `tasks.register("build")`, `include ':app'`).
    Definition,
    /// A reference site (`dependsOn("build")`, `libs.guava`, `project(":app")`).
    Reference,
}

/// One navigable token occurrence: its precise span, its symbol, and its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The precise source span of the referencing/declaring token.
    pub span: TextSpan,
    /// The symbol this occurrence names.
    pub symbol: Symbol,
    /// Whether this occurrence declares or references the symbol.
    pub role: OccurrenceRole,
}

/// The per-DSL node kinds the scanner navigates by.
struct Kinds {
    call: SyntaxKind,
    arg_list: SyntaxKind,
    block: SyntaxKind,
    path: SyntaxKind,
    kotlin_head: bool,
}

impl Kinds {
    fn for_lang(lang: DslLanguage) -> Kinds {
        match lang {
            DslLanguage::Kotlin => Kinds {
                call: kt::CALL,
                arg_list: kt::ARG_LIST,
                block: kt::BLOCK,
                path: kt::ACCESS_PATH,
                kotlin_head: true,
            },
            DslLanguage::Groovy => Kinds {
                call: gv::CALL,
                arg_list: gv::ARG_LIST,
                block: gv::CLOSURE,
                path: gv::PATH,
                kotlin_head: false,
            },
        }
    }
}

/// What a recognized call head means for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadKind {
    TaskRef,
    TaskDecl,
    Project,
    Include,
    Other,
}

/// Classifies a (scope-resolved) call head.
fn classify_head(head: &str) -> HeadKind {
    match head {
        "dependsOn" | "finalizedBy" | "mustRunAfter" | "shouldRunAfter" | "tasks.named"
        | "tasks.getByName" => HeadKind::TaskRef,
        "task" | "tasks.register" | "tasks.create" => HeadKind::TaskDecl,
        "project" => HeadKind::Project,
        "include" => HeadKind::Include,
        _ => HeadKind::Other,
    }
}

/// Scans `root` (the red-tree root of a `lang` document) for navigable occurrences.
pub fn collect_occurrences(root: &SyntaxNode, lang: DslLanguage) -> Vec<Occurrence> {
    let kinds = Kinds::for_lang(lang);
    let mut out = Vec::new();
    visit(root, None, &kinds, &mut out);
    out
}

/// Returns the smallest-span occurrence containing `offset`, if any.
pub fn locate_at(occurrences: &[Occurrence], offset: usize) -> Option<Occurrence> {
    occurrences
        .iter()
        .filter(|occ| occ.span.contains(offset))
        .min_by_key(|occ| occ.span.len)
        .cloned()
}

/// Descends `node`, processing call nodes and recursing through wrappers.
fn visit(node: &SyntaxNode, scope: Option<&str>, kinds: &Kinds, out: &mut Vec<Occurrence>) {
    for child in node.child_nodes() {
        if child.kind() == kinds.call {
            process_call(&child, scope, kinds, out);
        } else {
            visit(&child, scope, kinds, out);
        }
    }
}

/// Classifies one call and recurses into its block body and argument list.
fn process_call(call: &SyntaxNode, scope: Option<&str>, kinds: &Kinds, out: &mut Vec<Occurrence>) {
    let head = extract_head(call, kinds);
    let resolved = resolve_head(&head, scope);
    let arg_list = call.child_nodes().find(|n| n.kind() == kinds.arg_list);

    if let Some(args) = &arg_list {
        emit_from_args(classify_head(&resolved), args, kinds, out);
    }
    if let Some(block) = call.child_nodes().find(|n| n.kind() == kinds.block) {
        visit(&block, Some(last_segment(&head)), kinds, out);
    }
    if let Some(args) = &arg_list {
        visit(args, scope, kinds, out);
    }
}

/// Resolves a bare `register`/`named`/… head to `tasks.*` when inside a `tasks { }` block.
fn resolve_head(head: &str, scope: Option<&str>) -> String {
    if head.contains('.') {
        return head.to_string();
    }
    let in_tasks = scope.map(last_segment).is_some_and(|s| s == "tasks");
    if in_tasks && matches!(head, "register" | "create" | "named" | "getByName") {
        format!("tasks.{head}")
    } else {
        head.to_string()
    }
}

/// Returns the last `.`-segment of a dotted head.
fn last_segment(head: &str) -> &str {
    head.rsplit('.').next().unwrap_or(head)
}

/// Reads a call's head: the first `ACCESS_PATH` node (Kotlin) or leading IDENTs (Groovy).
fn extract_head(call: &SyntaxNode, kinds: &Kinds) -> String {
    if kinds.kotlin_head {
        return call
            .child_nodes()
            .find(|n| n.kind() == kinds.path)
            .map(|n| dotted_idents(&n))
            .unwrap_or_default();
    }
    let mut parts = Vec::new();
    for child in call.children() {
        match child {
            SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => {
                parts.push(t.text().to_string())
            }
            SyntaxElement::Token(t) if t.kind().is_trivia() || t.text() == "." => {}
            _ => break,
        }
    }
    parts.join(".")
}

/// Emits occurrences for a call's direct argument list, by head category.
fn emit_from_args(kind: HeadKind, args: &SyntaxNode, kinds: &Kinds, out: &mut Vec<Occurrence>) {
    scan_paths_and_nested(args, kinds, out);

    match kind {
        HeadKind::TaskRef => push_task(args, OccurrenceRole::Reference, out),
        HeadKind::TaskDecl => push_task(args, OccurrenceRole::Definition, out),
        HeadKind::Project => {
            if let Some((path, span)) = first_string(args) {
                out.push(Occurrence {
                    span,
                    symbol: Symbol::Project(path),
                    role: OccurrenceRole::Reference,
                });
            }
        }
        HeadKind::Include => {
            for (path, span) in all_strings(args) {
                out.push(Occurrence {
                    span,
                    symbol: Symbol::Project(path),
                    role: OccurrenceRole::Definition,
                });
            }
        }
        HeadKind::Other => {}
    }
}

/// Pushes a task-name occurrence from the first string/bare-ident name token, if present.
fn push_task(args: &SyntaxNode, role: OccurrenceRole, out: &mut Vec<Occurrence>) {
    if let Some((name, span)) = first_name_token(args) {
        out.push(Occurrence {
            span,
            symbol: Symbol::Task(name),
            role,
        });
    }
}

/// Scans an arg list for `libs.*` accessor paths and nested `project(...)` references.
///
/// Kotlin nests `project(":x")` as an `ACCESS_PATH` "project" + `ARG_LIST` sibling pair;
/// Groovy nests it as a bare `IDENT` "project" token + `ARG_LIST` sibling. Both are folded
/// here. A `libs.*` accessor is a path node with ≥1 segment after `libs`.
fn scan_paths_and_nested(args: &SyntaxNode, kinds: &Kinds, out: &mut Vec<Occurrence>) {
    let elems = args.children();
    let mut index = 0;
    while index < elems.len() {
        let head = project_head(&elems[index], kinds);
        if head.as_deref() == Some("project")
            && let Some(SyntaxElement::Node(next)) = elems.get(index + 1)
            && next.kind() == kinds.arg_list
        {
            if let Some((path, span)) = first_string(next) {
                out.push(Occurrence {
                    span,
                    symbol: Symbol::Project(path),
                    role: OccurrenceRole::Reference,
                });
            }
            index += 2;
            continue;
        }
        if let SyntaxElement::Node(node) = &elems[index]
            && node.kind() == kinds.path
            && let Some(rest) = accessor_rest(&dotted_idents(node))
        {
            out.push(Occurrence {
                span: node.span(),
                symbol: Symbol::CatalogAccessor(rest),
                role: OccurrenceRole::Reference,
            });
        }
        index += 1;
    }
}

/// Returns the call-head name an element carries when it could head a nested call.
///
/// A Kotlin head is a path node (`ACCESS_PATH`); a Groovy head is a bare `IDENT` token.
fn project_head(elem: &SyntaxElement, kinds: &Kinds) -> Option<String> {
    match elem {
        SyntaxElement::Node(node) if node.kind() == kinds.path => Some(dotted_idents(node)),
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => {
            Some(token.text().to_string())
        }
        _ => None,
    }
}

/// Strips the leading `libs` catalog segment, returning the dotted remainder (≥1 segment).
fn accessor_rest(dotted: &str) -> Option<String> {
    let rest = dotted.strip_prefix("libs.")?;
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

/// Returns the first string (preferred) or bare-ident name token: `(text, span)`.
fn first_name_token(args: &SyntaxNode) -> Option<(String, TextSpan)> {
    if let Some(found) = first_string(args) {
        return Some(found);
    }
    args.children().iter().find_map(|c| match c {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => {
            Some((t.text().to_string(), t.span()))
        }
        _ => None,
    })
}

/// Returns the first `STRING` token as `(unquoted, span)`.
fn first_string(args: &SyntaxNode) -> Option<(String, TextSpan)> {
    args.children().iter().find_map(string_of)
}

/// Returns every `STRING` token as `(unquoted, span)`.
fn all_strings(args: &SyntaxNode) -> Vec<(String, TextSpan)> {
    args.children().iter().filter_map(string_of).collect()
}

/// Maps a child element to `(unquoted, span)` if it is a `STRING` token.
fn string_of(child: &SyntaxElement) -> Option<(String, TextSpan)> {
    match child {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::STRING => {
            Some((unquote(t.text()), t.span()))
        }
        _ => None,
    }
}

/// Joins a path node's `IDENT` tokens with `.` (`libs.guava`, `tasks.register`).
fn dotted_idents(node: &SyntaxNode) -> String {
    node.children()
        .iter()
        .filter_map(|c| match c {
            SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Strips matching surrounding quotes from a string literal token.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    for quote in ['"', '\''] {
        if trimmed.len() >= 2 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}
