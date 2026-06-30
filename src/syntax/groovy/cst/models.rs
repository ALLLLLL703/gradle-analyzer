use crate::syntax::{groovy::models::GroovyTokenKind, span::TextSpan};

#[derive(Debug)]
pub struct GroovyCstNode {
    pub kind: GroovyCstNodeKind,
    pub span: TextSpan,
    pub children: Vec<GroovyCstElement>,
}

#[derive(Debug, strum_macros::Display)]
pub enum GroovyCstNodeKind {
    Document,
    Statement,
    Expression,
    PrimaryExpression,
    ParenthesizedExpression,
    MemberAccessExpression,
    CallExpression,
    CommandExpression,
    ArgumentList,
    Closure,
    AssignmentExpression,
    BinaryExpression,
    Error,
}

#[derive(Debug, strum_macros::Display)]
pub enum GroovyCstElement {
    Node(GroovyCstNode),
    Token(GroovyTokenKind),
}

pub struct GroovyCstDocument {
    pub root: GroovyCstNode,
    pub issues: Vec<GroovySyntaxIssue>,
}

pub enum GroovySyntaxIssueKind {
    UnexpectedClosingDelimiter,
    UnclosedBlock,
    UnexpectedToken,
}

pub struct GroovySyntaxIssue {
    pub span: TextSpan,
    pub kind: GroovySyntaxIssueKind,
}
