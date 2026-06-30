pub mod models;

use tower_lsp::lsp_types::Diagnostic;

use crate::syntax::{
    groovy::{
        cst::models::{GroovyCstDocument, GroovySyntaxIssue},
        models::{GroovyToken, GroovyTokenKind},
    },
    span::TextSpan,
};

pub struct Parser<'a> {
    tokens: &'a [GroovyToken],
    cursor: usize,
    issues: Vec<GroovySyntaxIssue>,
}

#[cfg(test)]
mod test {
    #[tokio::test]
    async fn test_parse_tokens() {}
}
