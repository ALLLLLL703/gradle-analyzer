//! Layer 3b: per-context candidate eligibility builders + static tables.
//!
//! [`collect_eligible`] is the single entry the engine calls between classification and
//! ranking. It dispatches on the [`CompletionContext`] block + position and emits
//! [`Candidate`]s in INSERTION order from two sources: small STATIC tables (block keywords,
//! plugin ids, repository functions, dependency configurations, a coordinate scaffold) and
//! the workspace-derived [`VisibleScope`] (task names, project paths, catalog accessors).
//!
//! This is the documented Task-16 enrichment seam: the advanced tier appends
//! [`CandidateKind::PluginContributed`] candidates to the vec this function returns, BEFORE
//! ranking, without changing classification or ordering logic. Every `detail` string is
//! rendered through the [`Translator`] so no user-facing text is hardcoded here.

use crate::i18n::MessageKey;

use super::context::{CompletionBlockContext, CompletionContext, CompletionPosition};
use super::scope::VisibleScope;
use super::{Candidate, CandidateKind, CompletionServices};

/// Top-level block keywords offered at the document root.
const BLOCK_KEYWORDS: &[&str] = &[
    "plugins",
    "dependencies",
    "repositories",
    "tasks",
    "subprojects",
    "allprojects",
    "buildscript",
];

/// A small static table of commonly-applied plugin ids (NOT plugin-contributed members).
const PLUGIN_IDS: &[&str] = &[
    "java",
    "java-library",
    "application",
    "java-gradle-plugin",
    "maven-publish",
    "org.jetbrains.kotlin.jvm",
    "org.jetbrains.kotlin.plugin.spring",
    "com.android.application",
    "com.android.library",
];

/// Standard Gradle repository functions.
const REPOSITORIES: &[&str] = &["mavenCentral", "google", "mavenLocal", "gradlePluginPortal"];

/// Common dependency configurations.
const CONFIGURATIONS: &[&str] = &[
    "implementation",
    "api",
    "compileOnly",
    "runtimeOnly",
    "testImplementation",
    "testRuntimeOnly",
    "annotationProcessor",
];

/// Collects the eligible candidates for `context` from the scope + static tables.
///
/// Returns candidates in INSERTION order (ranking happens separately in
/// [`super::ranking::rank`]). The order here is intentionally NOT the final display order —
/// keeping eligibility and ranking decoupled is what lets Task 16 append plugin-contributed
/// candidates without disturbing either.
pub fn collect_eligible(
    context: &CompletionContext,
    scope: &VisibleScope,
    services: &CompletionServices,
) -> Vec<Candidate> {
    // A `libs.` accessor site is block-independent: offer matching catalog accessors only.
    if let CompletionPosition::CatalogAccessor { typed } = &context.position {
        return catalog_candidates(scope, services, typed);
    }
    if context.position == CompletionPosition::TaskReference {
        return task_candidates(scope, services);
    }

    match context.block {
        CompletionBlockContext::TopLevel => block_keyword_candidates(services),
        CompletionBlockContext::Dependencies => dependency_candidates(scope, services),
        CompletionBlockContext::Plugins => plugin_candidates(scope, services),
        CompletionBlockContext::Repositories => repository_candidates(services),
        CompletionBlockContext::Tasks => task_candidates(scope, services),
        CompletionBlockContext::Other => Vec::new(),
    }
}

/// Top-level block keywords.
fn block_keyword_candidates(services: &CompletionServices) -> Vec<Candidate> {
    let detail = services.translator.text(MessageKey::CompletionDetailBlockKeyword);
    BLOCK_KEYWORDS
        .iter()
        .map(|kw| Candidate::new(*kw, CandidateKind::BlockKeyword, detail.clone()))
        .collect()
}

/// Inside `dependencies {`: configurations + a coordinate scaffold + catalog accessors +
/// project paths.
fn dependency_candidates(scope: &VisibleScope, services: &CompletionServices) -> Vec<Candidate> {
    let mut out = Vec::new();
    let config_detail = services
        .translator
        .text(MessageKey::CompletionDetailConfiguration);
    for config in CONFIGURATIONS {
        out.push(Candidate::new(
            *config,
            CandidateKind::DependencyConfiguration,
            config_detail.clone(),
        ));
    }
    let scaffold_detail = services
        .translator
        .text(MessageKey::CompletionDetailCoordinateScaffold);
    out.push(Candidate::with_insert(
        "implementation(\"group:artifact:version\")",
        CandidateKind::CoordinateScaffold,
        scaffold_detail,
        "implementation(\"$1:$2:$3\")",
    ));
    out.extend(catalog_candidates(scope, services, "libs."));
    out.extend(project_path_candidates(scope, services));
    out
}

/// Inside `plugins {`: the `id` helper + static plugin ids + catalog plugin accessors.
fn plugin_candidates(scope: &VisibleScope, services: &CompletionServices) -> Vec<Candidate> {
    let detail = services.translator.text(MessageKey::CompletionDetailPluginId);
    let mut out = vec![Candidate::with_insert(
        "id",
        CandidateKind::PluginId,
        detail.clone(),
        "id(\"$1\")",
    )];
    for plugin in PLUGIN_IDS {
        out.push(Candidate::new(*plugin, CandidateKind::PluginId, detail.clone()));
    }
    for plugin in &scope.buildsrc_plugins {
        out.push(Candidate::new(
            plugin.clone(),
            CandidateKind::PluginId,
            detail.clone(),
        ));
    }
    for accessor in &scope.catalog_accessors {
        if accessor.accessor.starts_with("libs.plugins.") {
            out.push(catalog_candidate(services, accessor));
        }
    }
    out
}

/// Inside `repositories {`: standard repository functions.
fn repository_candidates(services: &CompletionServices) -> Vec<Candidate> {
    let detail = services.translator.text(MessageKey::CompletionDetailRepository);
    REPOSITORIES
        .iter()
        .map(|repo| {
            Candidate::with_insert(
                *repo,
                CandidateKind::Repository,
                detail.clone(),
                format!("{repo}()"),
            )
        })
        .collect()
}

/// Inside `tasks {` or a task-reference site: visible task names.
fn task_candidates(scope: &VisibleScope, services: &CompletionServices) -> Vec<Candidate> {
    let detail = services.translator.text(MessageKey::CompletionDetailTaskName);
    scope
        .task_names
        .iter()
        .map(|name| Candidate::new(name.clone(), CandidateKind::TaskName, detail.clone()))
        .collect()
}

/// Project-path candidates from the graph (`:app`, `:core`).
fn project_path_candidates(scope: &VisibleScope, services: &CompletionServices) -> Vec<Candidate> {
    let detail = services
        .translator
        .text(MessageKey::CompletionDetailProjectPath);
    scope
        .project_paths
        .iter()
        .map(|path| {
            Candidate::with_insert(
                format!("project(\"{path}\")"),
                CandidateKind::ProjectPath,
                detail.clone(),
                format!("project(\"{path}\")"),
            )
        })
        .collect()
}

/// Catalog-accessor candidates whose accessor starts with `typed`.
///
/// The label is always the FULL accessor (e.g. `libs.guava`); the LSP client computes the
/// replace range from the typed prefix, so no insert-text override is needed and the
/// candidate stays correct across editors.
fn catalog_candidates(
    scope: &VisibleScope,
    services: &CompletionServices,
    typed: &str,
) -> Vec<Candidate> {
    scope
        .catalog_accessors
        .iter()
        .filter(|a| a.accessor.starts_with(typed))
        .map(|accessor| catalog_candidate(services, accessor))
        .collect()
}

/// Builds one catalog-accessor candidate (full-accessor label, localized detail).
fn catalog_candidate(
    services: &CompletionServices,
    accessor: &super::scope::CatalogAccessor,
) -> Candidate {
    let detail = services.translator.get_text(
        MessageKey::CompletionDetailCatalogAccessor,
        &[&accessor.target],
    );
    Candidate::new(accessor.accessor.clone(), CandidateKind::CatalogAccessor, detail)
}
