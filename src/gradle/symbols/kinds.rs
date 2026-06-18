//! Per-DSL [`SyntaxKind`] bundle so the walker reads one vocabulary for both frontends.
//!
//! The Kotlin and Groovy frontends allocate their custom kinds from the same numeric base,
//! so a raw `CALL` value alone cannot tell them apart — the DSL must be known. [`DslKinds`]
//! captures the handful of kinds the outline builder matches on for a given
//! [`DslLanguage`], plus an `is_kotlin` flag for the few places the tree SHAPE (not just the
//! kind) differs. Centralizing the mapping keeps the builder free of `parser::kotlin` /
//! `parser::groovy` paths scattered through its match arms.

use crate::gradle::parser::{groovy, kotlin};
use crate::gradle::syntax::SyntaxKind;
use crate::gradle::workspace::DslLanguage;

/// The outline-relevant syntax kinds for one DSL.
#[derive(Debug, Clone, Copy)]
pub struct DslKinds {
    /// `true` for Kotlin, `false` for Groovy (selects tree-shape handling).
    pub is_kotlin: bool,
    /// A call statement (`plugins { }`, `implementation(...)`).
    pub call: SyntaxKind,
    /// A property assignment (`group = "..."`).
    pub assignment: SyntaxKind,
    /// A brace block / closure body.
    pub block: SyntaxKind,
    /// An argument list.
    pub arg_list: SyntaxKind,
    /// A named argument (`plugin: 'x'`).
    pub named_arg: SyntaxKind,
    /// Kotlin's dotted access path (unused for Groovy; set to a harmless value there).
    pub access_path: SyntaxKind,
    /// Groovy's dotted path node (unused for Kotlin; set to a harmless value there).
    pub path: SyntaxKind,
    /// Groovy's declaration wrapper (`def x = ...`); Kotlin has no equivalent wrapper.
    pub declaration: SyntaxKind,
}

impl DslKinds {
    /// Builds the kind bundle for `language`.
    pub fn for_language(language: DslLanguage) -> DslKinds {
        match language {
            DslLanguage::Kotlin => DslKinds {
                is_kotlin: true,
                call: kotlin::kinds::CALL,
                assignment: kotlin::kinds::ASSIGNMENT,
                block: kotlin::kinds::BLOCK,
                arg_list: kotlin::kinds::ARG_LIST,
                named_arg: kotlin::kinds::NAMED_ARG,
                access_path: kotlin::kinds::ACCESS_PATH,
                // Kotlin has no PATH node; map to ACCESS_PATH so accessor lookups still work.
                path: kotlin::kinds::ACCESS_PATH,
                // Kotlin has no declaration wrapper; use a kind it never produces here.
                declaration: kotlin::kinds::PACKAGE,
            },
            DslLanguage::Groovy => DslKinds {
                is_kotlin: false,
                call: groovy::CALL,
                assignment: groovy::ASSIGNMENT,
                block: groovy::CLOSURE,
                arg_list: groovy::ARG_LIST,
                named_arg: groovy::NAMED_ARG,
                // Groovy has no ACCESS_PATH; map to PATH so accessor lookups still work.
                access_path: groovy::PATH,
                path: groovy::PATH,
                declaration: groovy::DECLARATION,
            },
        }
    }
}
