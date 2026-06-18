//! Collecting every reference site for a located [`Symbol`] in the current document.
//!
//! Works from a reference *or* a declaration position: given the [`Occurrence`]s already
//! scanned for the document, it returns every occurrence that shares the located [`Symbol`]
//! (declaration included, so the result is a useful editor "find all references"). Scope is
//! the current document only — a cross-document sweep is out of Task 12 scope.

use super::locate::{Occurrence, Symbol};
use super::{NavDocument, NavTarget};

/// Returns every occurrence in `occurrences` that names `symbol`, as local targets.
pub fn collect_references(
    doc: &NavDocument,
    symbol: &Symbol,
    occurrences: &[Occurrence],
) -> Vec<NavTarget> {
    occurrences
        .iter()
        .filter(|occ| &occ.symbol == symbol)
        .map(|occ| NavTarget::local(doc.id().clone(), occ.span))
        .collect()
}
