use logos::Logos;
use strum_macros::EnumString;

use crate::syntax::span::{TextLocation, TextSpan};

#[derive(logos::Logos, PartialEq, Eq, Debug, EnumString, strum_macros::Display, Clone, Default)]
#[logos(skip r"[ \t\n\r\f]+")]
pub enum GroovyTokenKind {
    #[regex(r"//[^\n\r]*", allow_greedy = true)]
    LineComment,
    #[regex(r"/\*([^*]|\*+[^*/])*\*+/")]
    BlockComment,

    #[token("abstract")]
    Abstract,
    #[token("as")]
    As,
    #[token("assert")]
    Assert,
    #[token("break")]
    Break,
    #[token("case")]
    Case,
    #[token("catch")]
    Catch,
    #[token("class")]
    Class,
    #[token("const")]
    Const,
    #[token("continue")]
    Continue,
    #[token("def")]
    Def,
    #[token("default")]
    Default,
    #[token("do")]
    Do,
    #[token("else")]
    Else,
    #[token("enum")]
    Enum,
    #[token("extends")]
    Extends,
    #[token("final")]
    Final,
    #[token("finally")]
    Finally,
    #[token("for")]
    For,
    #[token("goto")]
    Goto,
    #[token("if")]
    If,
    #[token("implements")]
    Implements,
    #[token("import")]
    Import,
    #[token("in")]
    In,
    #[token("instanceof")]
    InstanceOf,
    #[token("interface")]
    Interface,
    #[token("native")]
    Native,
    #[token("new")]
    New,
    #[token("non-sealed")]
    NonSealed,
    #[token("package")]
    Package,
    #[token("public")]
    Public,
    #[token("protected")]
    Protected,
    #[token("private")]
    Private,
    #[token("record")]
    Record,
    #[token("return")]
    Return,
    #[token("sealed")]
    Sealed,
    #[token("static")]
    Static,
    #[token("strictfp")]
    Strictfp,
    #[token("super")]
    Super,
    #[token("switch")]
    Switch,
    #[token("synchronized")]
    Synchronized,
    #[token("this")]
    This,
    #[token("threadsafe")]
    ThreadSafe,
    #[token("throw")]
    Throw,
    #[token("throws")]
    Throws,
    #[token("trait")]
    Trait,
    #[token("transient")]
    Transient,
    #[token("try")]
    Try,
    #[token("var")]
    Var,
    #[token("void")]
    Void,
    #[token("volatile")]
    Volatile,
    #[token("while")]
    While,
    #[token("yield")]
    Yield,

    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("null")]
    Null,

    #[regex(r#"\"([^\"$\\\r\n]|\\.)*\$([A-Za-z_][A-Za-z0-9_]*|\{[^}\r\n]*\})([^\"\\\r\n]|\\.)*\""#)]
    GString,
    #[regex(r#"'''([^'\\]|\\.|'{1,2}[^'])*'''"#)]
    TripleSingleQuotedString,
    #[regex(r#"\"\"\"([^\"\\]|\\.|\"{1,2}[^\"])*\"\"\""#)]
    TripleDoubleQuotedString,
    #[regex(r#"'([^'\\\r\n]|\\.)*'"#)]
    SingleQuotedString,
    #[regex(r#"\"([^\"\\\r\n]|\\.)*\""#)]
    DoubleQuotedString,

    #[regex(r"0[xX][0-9A-Fa-f_]+[GgIiLl]?")]
    HexInteger,
    #[regex(r"0[bB][01_]+[GgIiLl]?")]
    BinaryInteger,
    #[regex(r"((([0-9][0-9_]*)?\.[0-9][0-9_]*)|([0-9][0-9_]*\.))([eE][+-]?[0-9][0-9_]*)?[GgDdFf]?")]
    DecimalFloat,
    #[regex(r"[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*[GgDdFf]?")]
    ExponentFloat,
    #[regex(r"[0-9][0-9_]*[GgIiLl]?")]
    DecimalInteger,

    #[token("?.")]
    SafeDot,
    #[token("*.")]
    SpreadDot,
    #[token("..")]
    RangeInclusive,
    #[token("..<")]
    RangeExclusiveRight,
    #[token("<..")]
    RangeExclusiveLeft,
    #[token("<..<")]
    RangeExclusive,
    #[token("?:")]
    Elvis,
    #[token("::")]
    MethodReference,
    #[token("->")]
    Arrow,
    #[token("=>")]
    ClosureArrow,

    #[token("===")]
    IdentityEquals,
    #[token("!==")]
    IdentityNotEquals,
    #[token("==~")]
    RegexMatches,
    #[token("=~")]
    RegexFind,
    #[token("==")]
    EqualsEquals,
    #[token("!=")]
    NotEquals,
    #[token("<=")]
    LessEquals,
    #[token(">=")]
    GreaterEquals,
    #[token("<=>")]
    Spaceship,

    #[token("++")]
    PlusPlus,
    #[token("--")]
    MinusMinus,
    #[token("**=")]
    PowerAssign,
    #[token("**")]
    Power,
    #[token("+=")]
    PlusAssign,
    #[token("-=")]
    MinusAssign,
    #[token("*=")]
    StarAssign,
    #[token("/=")]
    SlashAssign,
    #[token("%=")]
    PercentAssign,
    #[token("&=")]
    AmpersandAssign,
    #[token("|=")]
    PipeAssign,
    #[token("^=")]
    CaretAssign,

    #[token("<<=")]
    ShiftLeftAssign,
    #[token(">>>=")]
    UnsignedShiftRightAssign,
    #[token(">>=")]
    ShiftRightAssign,
    #[token("<<")]
    ShiftLeft,
    #[token(">>>")]
    UnsignedShiftRight,
    #[token(">>")]
    ShiftRight,

    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("!")]
    Bang,
    #[token("~")]
    Tilde,
    #[token("&")]
    Ampersand,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("=")]
    Equals,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("?")]
    Question,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,

    #[token("(")]
    ParenthesesLeft,
    #[token(")")]
    ParenthesesRight,
    #[token("{")]
    BraceLeft,
    #[token("}")]
    BraceRight,
    #[token("[")]
    BracketLeft,
    #[token("]")]
    BracketRight,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token("@")]
    At,

    #[regex(r"[A-Za-z_$][A-Za-z0-9_$]*")]
    Identifier,

    #[default]
    #[regex(r".", priority = 0)]
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GroovyToken {
    pub kind: GroovyTokenKind,
    pub lexeme: String,
    pub span: TextSpan,
}

impl GroovyToken {
    pub fn parse_string_to_token(contents: &str) -> Vec<Self> {
        let mut result = Vec::new();
        let mut lexer = GroovyTokenKind::lexer(contents);
        let cursor_byte = 0;
        let cursor_loc = TextLocation::new(0, 0);
        for (groovy_token_kind, range) in lexer.spanned() {
            let byte_start = range.start;
            let byte_end = range.end;
            let start_location = advance_loc(cursor_loc, &contents[cursor_byte..byte_start]);
            let end_location = advance_loc(start_location, &contents[byte_start..byte_end]);
            result.push(GroovyToken {
                kind: groovy_token_kind.unwrap_or_default(),
                lexeme: contents[byte_start..byte_end].to_string(),
                span: TextSpan {
                    start_bytes: byte_start,
                    end_bytes: byte_end,
                    start_char: start_location,
                    end_char: end_location,
                },
            });
        }
        result
    }
}

fn advance_loc(mut loc: TextLocation, text: &str) -> TextLocation {
    for ch in text.chars() {
        if ch == '\n' {
            loc.col += 1;
            loc.row = 0;
        } else {
            loc.row += 1;
        }
    }
    loc
}
#[cfg(test)]
mod test {
    use std::time::Duration;

    use logos::Logos;
    use tokio::sync::oneshot::error::TryRecvError;

    use crate::syntax::groovy::models::GroovyTokenKind;

    // #[tokio::test]
    async fn test_skinmod_gradle_file() {
        let file_path = "/home/sanae/CodeProject/csharp/mods/slayTheSpire2/skinmod/build.gradle";
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let content_result = tokio::fs::read_to_string(file_path).await;
            match &content_result {
                Ok(_) => println!("success to read the test build.gradle"),
                Err(_) => println!("failed to read the test build.gradle"),
            }
            tx.send(content_result.unwrap_or_default()).unwrap();
        });
        let mut lexer;
        let mut content: String;
        'l: loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            match rx.try_recv() {
                Ok(c) => {
                    content = c.clone();
                    lexer = GroovyTokenKind::lexer(&content);
                    break 'l;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Closed) => {
                    println!("channel closed ,,,");
                }
            }
        }
        for next in lexer {
            println!("[token = {}]", next.unwrap());
        }
    }
}
