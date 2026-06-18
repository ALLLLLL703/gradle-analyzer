//! The red-tree walker that builds the [`SymbolNode`] backbone.
//!
//! Walks the top level of a parsed build script and classifies each call/assignment by its
//! leading name into outline entries, recursing into recognized container blocks
//! (`plugins`, `repositories`, `dependencies`, `tasks`, and the major script sections). The
//! walk is deliberately tolerant: `OPAQUE`/`ERROR_NODE` subtrees are skipped, an unknown
//! top-level block still yields a generic [`OutlineKind::Block`] so structure survives, and
//! an unclosed block (whose later siblings the parser folded inside it) simply nests those
//! entries under the open block — partial but never empty and never noisy.

use std::rc::Rc;

use crate::gradle::syntax::{SyntaxKind, SyntaxNode};

use super::kinds::DslKinds;
use super::naming;
use super::node::{OutlineKind, SymbolNode};

/// The classification context a block's children are walked under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// Top level of the script (or a generic section): full dispatch.
    Script,
    /// Inside `plugins { }`: children are individual plugin applications.
    Plugins,
    /// Inside `repositories { }`: children are repositories.
    Repositories,
    /// Inside `dependencies { }`: children are dependency declarations.
    Dependencies,
    /// Inside a `tasks { }` block: children are task configurations.
    Tasks,
}

/// Builds the outline for a parsed script `root` under the given DSL `kinds`.
pub fn build(root: &Rc<SyntaxNode>, kinds: &DslKinds) -> Vec<SymbolNode> {
    walk_children(root, kinds, Context::Script)
}

/// Walks the statement children of `parent`, classifying each under `context`.
fn walk_children(parent: &Rc<SyntaxNode>, kinds: &DslKinds, context: Context) -> Vec<SymbolNode> {
    let mut out = Vec::new();
    for child in parent.child_nodes() {
        let kind = child.kind();
        if kind == SyntaxKind::OPAQUE || kind == SyntaxKind::ERROR_NODE {
            continue;
        }
        // Groovy wraps decorated statements in a DECLARATION node; unwrap to the inner call.
        if kind == kinds.declaration && !kinds.is_kotlin {
            out.extend(walk_children(&child, kinds, context));
            continue;
        }
        if kind == kinds.call {
            if let Some(symbol) = classify_call(&child, kinds, context) {
                out.push(symbol);
            }
        } else if kind == kinds.assignment
            && context == Context::Script
            && let Some(symbol) = classify_assignment(&child, kinds)
        {
            out.push(symbol);
        }
    }
    out
}

/// Classifies a `CALL` node into an outline entry, depending on the surrounding context.
fn classify_call(node: &Rc<SyntaxNode>, kinds: &DslKinds, context: Context) -> Option<SymbolNode> {
    match context {
        Context::Plugins => Some(plugin_symbol(node, kinds)),
        Context::Repositories => Some(repository_symbol(node, kinds)),
        Context::Dependencies => Some(dependency_symbol(node, kinds)),
        Context::Tasks => Some(task_config_symbol(node, kinds)),
        Context::Script => classify_top_level(node, kinds),
    }
}

/// Classifies a top-level call by its leading name into a section, task, include, or block.
fn classify_top_level(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<SymbolNode> {
    let segments = naming::call_name_segments(node, kinds);
    let head = segments.first().map(String::as_str).unwrap_or("");
    let selection = naming::name_selection_span(node, kinds);

    match head {
        "plugins" | "pluginManagement" => {
            Some(container(node, head, OutlineKind::Block, selection, kinds, Context::Plugins))
        }
        "repositories" => {
            Some(container(node, head, OutlineKind::Block, selection, kinds, Context::Repositories))
        }
        "dependencies" => {
            Some(container(node, head, OutlineKind::Block, selection, kinds, Context::Dependencies))
        }
        "buildscript" | "allprojects" | "subprojects" | "dependencyResolutionManagement" => {
            Some(container(node, head, OutlineKind::Section, selection, kinds, Context::Script))
        }
        "tasks" => Some(tasks_entry(node, &segments, kinds, selection)),
        "task" if !kinds.is_kotlin => Some(groovy_task_symbol(node, kinds, selection)),
        "include" => include_symbol(node, kinds, selection),
        "apply" if !kinds.is_kotlin => apply_symbol(node, kinds, selection),
        _ => generic_block(node, head, kinds, selection),
    }
}

/// Builds a container node by recursively walking `node`'s block body under `inner`.
fn container(
    node: &Rc<SyntaxNode>,
    name: &str,
    kind: OutlineKind,
    selection: crate::gradle::syntax::TextSpan,
    kinds: &DslKinds,
    inner: Context,
) -> SymbolNode {
    let children = match block_body(node, kinds) {
        Some(body) => walk_children(&body, kinds, inner),
        None => Vec::new(),
    };
    SymbolNode::container(name, kind, trimmed_span(node), selection, children)
}

/// Handles a `tasks.*` call: nested-call registration (`tasks.register("x")`) becomes a task
/// symbol; a `tasks { }` configuration block recurses for its task children.
fn tasks_entry(
    node: &Rc<SyntaxNode>,
    segments: &[String],
    kinds: &DslKinds,
    selection: crate::gradle::syntax::TextSpan,
) -> SymbolNode {
    let method = segments.get(1).map(String::as_str);
    match method {
        Some("register" | "named" | "create" | "getByName" | "replace") => {
            let name = naming::first_string_arg(node, kinds).unwrap_or_else(|| "<task>".to_string());
            SymbolNode::leaf(name, None, OutlineKind::Task, trimmed_span(node), selection)
        }
        _ => container(node, "tasks", OutlineKind::Section, selection, kinds, Context::Tasks),
    }
}

/// Builds a task symbol for a configuration inside a `tasks { }` block.
fn task_config_symbol(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> SymbolNode {
    let segments = naming::call_name_segments(node, kinds);
    let method = segments.first().map(String::as_str);
    let name = match method {
        Some("register" | "named" | "create" | "getByName") => {
            naming::first_string_arg(node, kinds)
        }
        _ => segments.first().cloned(),
    }
    .unwrap_or_else(|| "<task>".to_string());
    let selection = naming::name_selection_span(node, kinds);
    SymbolNode::leaf(name, None, OutlineKind::Task, trimmed_span(node), selection)
}

/// Builds a task symbol for the Groovy `task foo {}` form.
fn groovy_task_symbol(
    node: &Rc<SyntaxNode>,
    kinds: &DslKinds,
    selection: crate::gradle::syntax::TextSpan,
) -> SymbolNode {
    let name = naming::groovy_task_name(node, kinds).unwrap_or_else(|| "<task>".to_string());
    SymbolNode::leaf(name, None, OutlineKind::Task, trimmed_span(node), selection)
}

/// Builds a plugin symbol from a `plugins { }` child (`id("java")`, `kotlin("jvm")`, `alias`).
fn plugin_symbol(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> SymbolNode {
    let segments = naming::call_name_segments(node, kinds);
    let head = segments.first().cloned().unwrap_or_default();
    let coordinate = naming::first_string_arg(node, kinds)
        .or_else(|| naming::first_accessor_arg(node, kinds));
    let name = match (head.as_str(), &coordinate) {
        ("id" | "alias", Some(coord)) => coord.clone(),
        ("kotlin", Some(coord)) => format!("kotlin(\"{coord}\")"),
        (_, Some(coord)) => coord.clone(),
        (other, None) if !other.is_empty() => other.to_string(),
        _ => "<plugin>".to_string(),
    };
    let selection = naming::name_selection_span(node, kinds);
    SymbolNode::leaf(name, None, OutlineKind::Plugin, trimmed_span(node), selection)
}

/// Builds a repository symbol from a `repositories { }` child (`mavenCentral()`, `maven { }`).
fn repository_symbol(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> SymbolNode {
    let segments = naming::call_name_segments(node, kinds);
    let name = segments.first().cloned().unwrap_or_else(|| "<repository>".to_string());
    let selection = naming::name_selection_span(node, kinds);
    SymbolNode::leaf(name, None, OutlineKind::Repository, trimmed_span(node), selection)
}

/// Builds a dependency symbol from a `dependencies { }` child (`implementation("g:a:v")`).
///
/// The symbol NAME is the configuration (`implementation`); the coordinate (string,
/// `libs.*` accessor, or `project(...)`) goes into the detail so an editor shows both.
fn dependency_symbol(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> SymbolNode {
    let segments = naming::call_name_segments(node, kinds);
    let configuration = segments.first().cloned().unwrap_or_else(|| "<dependency>".to_string());
    let coordinate = naming::first_string_arg(node, kinds)
        .or_else(|| naming::first_project_ref_arg(node, kinds))
        .or_else(|| naming::first_accessor_arg(node, kinds));
    let selection = naming::name_selection_span(node, kinds);
    SymbolNode::leaf(configuration, coordinate, OutlineKind::Dependency, trimmed_span(node), selection)
}

/// Builds project-include symbols (`include(":app", ":core")`) — one per string argument.
fn include_symbol(
    node: &Rc<SyntaxNode>,
    kinds: &DslKinds,
    selection: crate::gradle::syntax::TextSpan,
) -> Option<SymbolNode> {
    let path = naming::first_string_arg(node, kinds)?;
    Some(SymbolNode::leaf(path, None, OutlineKind::Project, trimmed_span(node), selection))
}

/// Builds a plugin symbol from a Groovy `apply plugin: 'x'` call.
fn apply_symbol(
    node: &Rc<SyntaxNode>,
    kinds: &DslKinds,
    selection: crate::gradle::syntax::TextSpan,
) -> Option<SymbolNode> {
    let plugin = naming::named_arg_value(node, "plugin", kinds)?;
    Some(SymbolNode::leaf(plugin, None, OutlineKind::Plugin, trimmed_span(node), selection))
}

/// Falls back to a generic block symbol for an unrecognized top-level call WITH a block body.
///
/// A bare call with no block (e.g. a stray statement) is dropped to keep the outline
/// low-noise; only structure-bearing blocks survive as generic entries.
fn generic_block(
    node: &Rc<SyntaxNode>,
    head: &str,
    kinds: &DslKinds,
    selection: crate::gradle::syntax::TextSpan,
) -> Option<SymbolNode> {
    let body = block_body(node, kinds)?;
    if head.is_empty() {
        return None;
    }
    let children = walk_children(&body, kinds, Context::Script);
    Some(SymbolNode::container(head, OutlineKind::Block, trimmed_span(node), selection, children))
}

/// Builds a property symbol for `group = "..."` / `version = "..."` assignments.
fn classify_assignment(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<SymbolNode> {
    let segments = naming::call_name_segments(node, kinds);
    let head = segments.last().cloned().unwrap_or_default();
    if !matches!(head.as_str(), "group" | "version" | "name" | "description") {
        return None;
    }
    let value = naming::first_string_value(node);
    let selection = naming::name_selection_span(node, kinds);
    Some(SymbolNode::leaf(head, value, OutlineKind::Property, trimmed_span(node), selection))
}

/// Returns the block/closure body child of a call, if present.
fn block_body(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<Rc<SyntaxNode>> {
    node.child_nodes().find(|child| child.kind() == kinds.block)
}

/// Returns `node`'s span with leading/trailing whitespace excluded.
///
/// A statement node's span starts at the newline trivia that precedes it (the frontends
/// attach leading trivia to the following statement). LSP wants a symbol `range` to cover
/// the construct WITHOUT that leading/trailing whitespace, so the editor highlights the
/// statement itself rather than from the end of the previous line.
fn trimmed_span(node: &Rc<SyntaxNode>) -> crate::gradle::syntax::TextSpan {
    let span = node.span();
    let text = node.text();
    let leading = text.len() - text.trim_start().len();
    let trailing = text.len() - text.trim_end().len();
    let start = span.start + leading;
    let end = span.end().saturating_sub(trailing).max(start);
    crate::gradle::syntax::TextSpan::from_range(start, end)
}
