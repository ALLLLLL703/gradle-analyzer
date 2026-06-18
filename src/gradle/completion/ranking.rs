//! Layer 4: the SEPARATE deterministic ranking + cap pass.
//!
//! Eligibility ([`super::candidates::collect_eligible`]) decides WHICH candidates apply;
//! this module decides their ORDER and count, and nothing else. Keeping the two passes
//! decoupled is a hard design rule: [`rank`] never inspects the completion context, never
//! filters by relevance, and never invents candidates — it only sorts an already-eligible
//! set by `(group rank, label)` and truncates to `max_candidates`. That separation is what
//! lets Task 16 append plugin-contributed candidates to the eligible vec without touching
//! ordering logic, and makes the order a pure, testable function of the input set.

use super::{Candidate, CandidateKind};

/// Orders `candidates` deterministically by `(group rank, label)` and caps to `max`.
///
/// The group rank is the [`CandidateKind`] discriminant order (declaration order), so a
/// more context-specific kind precedes a generic one; ties break by label. The sort is
/// stable and total, so identical input always yields identical output. `max` of 0 yields
/// an empty result (the cap is applied verbatim; the caller validates it as positive).
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::completion::{Candidate, CandidateKind};
/// use gradle_analyzer::gradle::completion::ranking::rank;
///
/// // Eligibility order is arbitrary; ranking imposes (kind, label).
/// let eligible = vec![
///     Candidate::new("repositories", CandidateKind::BlockKeyword, ""),
///     Candidate::new("api", CandidateKind::DependencyConfiguration, ""),
///     Candidate::new("dependencies", CandidateKind::BlockKeyword, ""),
/// ];
/// let ranked = rank(eligible, 10);
/// let labels: Vec<_> = ranked.iter().map(|c| c.label.as_str()).collect();
/// // BlockKeyword (rank 0) sorts before DependencyConfiguration (rank 1); ties by label.
/// assert_eq!(labels, ["dependencies", "repositories", "api"]);
/// ```
pub fn rank(mut candidates: Vec<Candidate>, max: usize) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        group_rank(a.kind)
            .cmp(&group_rank(b.kind))
            .then_with(|| a.label.cmp(&b.label))
    });
    candidates.truncate(max);
    candidates
}

/// Returns the stable group rank for a [`CandidateKind`] (lower sorts first).
fn group_rank(kind: CandidateKind) -> u8 {
    kind as u8
}
