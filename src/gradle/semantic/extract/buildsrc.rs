//! buildSrc / convention-plugin contributed local symbols (STATIC visibility only).
//!
//! A buildSrc script (or a precompiled-script convention plugin) makes the task and plugin
//! names it DECLARES statically usable elsewhere in the build. This pass derives those
//! symbols from the task/plugin facts already extracted for the same document and records a
//! [`FactPayload::BuildSrcSymbol`] for each — names only, nothing executed. Only registered
//! tasks (not `named` reconfigurations) and applied plugin ids contribute a symbol.

use crate::gradle::semantic::facts::{BuildSrcSymbolKind, FactPayload, FactStatus};

use super::Emitter;

/// Records a buildSrc symbol fact for each declared task / plugin already extracted.
pub(super) fn contribute(emitter: &mut Emitter) {
    let symbols: Vec<(String, BuildSrcSymbolKind, crate::gradle::syntax::TextSpan)> = emitter
        .facts
        .iter()
        .filter_map(|fact| match &fact.payload {
            FactPayload::Task { name, registered: true } if !name.is_empty() => {
                Some((name.clone(), BuildSrcSymbolKind::Task, fact.metadata.source))
            }
            FactPayload::Plugin { id, apply: true, .. } if !id.is_empty() => {
                Some((id.clone(), BuildSrcSymbolKind::Plugin, fact.metadata.source))
            }
            _ => None,
        })
        .collect();

    for (name, symbol, source) in symbols {
        let key = name.clone();
        emitter.push(
            &key,
            None,
            source,
            FactStatus::Complete,
            FactPayload::BuildSrcSymbol { name, symbol },
        );
    }
}
