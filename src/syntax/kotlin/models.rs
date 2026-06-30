#[derive(logos::Logos, PartialEq, Debug, strum_macros::Display)]
#[logos(skip r"[ \t\n\r\f]+")]
pub enum KotlinToken {
    #[regex(r"//[^\n\r]*", allow_greedy = true)]
    LineComment,
    #[regex(r"/\*([^*]|\*+[^*/])*\*+/")]
    BlockComment,

    #[token("as?")]
    AsNullable,
    #[token("as")]
    As,
    #[token("break")]
    Break,
    #[token("class")]
    Class,
    #[token("continue")]
    Continue,
    #[token("do")]
    Do,
    #[token("else")]
    Else,
    #[token("false")]
    False,
    #[token("for")]
    For,
    #[token("fun")]
    Fun,
    #[token("if")]
    If,
    #[token("in")]
    In,
    #[token("!in")]
    NotIn,
    #[token("interface")]
    Interface,
    #[token("is")]
    Is,
    #[token("!is")]
    NotIs,
    #[token("null")]
    Null,
    #[token("object")]
    Object,
    #[token("package")]
    Package,
    #[token("return")]
    Return,
    #[token("super")]
    Super,
    #[token("this")]
    This,
    #[token("throw")]
    Throw,
    #[token("true")]
    True,
    #[token("try")]
    Try,
    #[token("typealias")]
    TypeAlias,
    #[token("typeof")]
    TypeOf,
    #[token("val")]
    Val,
    #[token("var")]
    Var,
    #[token("when")]
    When,
    #[token("while")]
    While,

    #[token("actual")]
    Actual,
    #[token("annotation")]
    Annotation,
    #[token("by")]
    By,
    #[token("catch")]
    Catch,
    #[token("companion")]
    Companion,
    #[token("const")]
    Const,
    #[token("constructor")]
    Constructor,
    #[token("crossinline")]
    CrossInline,
    #[token("data")]
    Data,
    #[token("delegate")]
    Delegate,
    #[token("dynamic")]
    Dynamic,
    #[token("enum")]
    Enum,
    #[token("expect")]
    Expect,
    #[token("external")]
    External,
    #[token("field")]
    Field,
    #[token("file")]
    File,
    #[token("final")]
    Final,
    #[token("finally")]
    Finally,
    #[token("get")]
    Get,
    #[token("import")]
    Import,
    #[token("infix")]
    Infix,
    #[token("init")]
    Init,
    #[token("inline")]
    Inline,
    #[token("inner")]
    Inner,
    #[token("internal")]
    Internal,
    #[token("it")]
    It,
    #[token("lateinit")]
    LateInit,
    #[token("noinline")]
    NoInline,
    #[token("open")]
    Open,
    #[token("operator")]
    Operator,
    #[token("out")]
    Out,
    #[token("override")]
    Override,
    #[token("param")]
    Param,
    #[token("private")]
    Private,
    #[token("property")]
    Property,
    #[token("protected")]
    Protected,
    #[token("public")]
    Public,
    #[token("receiver")]
    Receiver,
    #[token("reified")]
    Reified,
    #[token("sealed")]
    Sealed,
    #[token("set")]
    Set,
    #[token("setparam")]
    SetParam,
    #[token("suspend")]
    Suspend,
    #[token("tailrec")]
    TailRec,
    #[token("vararg")]
    VarArg,
    #[token("where")]
    Where,

    #[regex(r#"\"\"\"([^\"$]|\"{1,2}[^\"]|\$[A-Za-z_][A-Za-z0-9_]*|\$\{[^}]*\})*\"\"\""#)]
    MultilineString,
    #[regex(r#"\"([^\"$\\\r\n]|\\.)*\$([A-Za-z_][A-Za-z0-9_]*|\{[^}\r\n]*\})([^\"\\\r\n]|\\.)*\""#)]
    GString,
    #[regex(r#"\"([^\"\\\r\n]|\\.)*\""#)]
    String,
    #[regex(r#"'([^'\\\r\n]|\\.)'"#)]
    Character,

    #[regex(r"0[xX][0-9A-Fa-f_]+[uU]?[lL]?")]
    HexInteger,
    #[regex(r"0[bB][01_]+[uU]?[lL]?")]
    BinaryInteger,
    #[regex(r"((([0-9][0-9_]*)?\.[0-9][0-9_]*)|([0-9][0-9_]*\.))([eE][+-]?[0-9][0-9_]*)?[fF]?")]
    DecimalFloat,
    #[regex(r"[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*[fF]?")]
    ExponentFloat,
    #[regex(r"[0-9][0-9_]*[uU]?[lL]?")]
    DecimalInteger,

    #[token("?.")]
    SafeDot,
    #[token("*.")]
    SpreadDot,
    #[token("..")]
    RangeInclusive,
    #[token("..<")]
    RangeExclusive,
    #[token("?:")]
    Elvis,
    #[token("::")]
    DoubleColon,
    #[token("->")]
    Arrow,
    #[token("=>")]
    DoubleArrow,

    #[token("===")]
    TripleEquals,
    #[token("!==")]
    TripleNotEquals,
    #[token("==")]
    EqualsEquals,
    #[token("!=")]
    NotEquals,
    #[token("<=")]
    LessEquals,
    #[token(">=")]
    GreaterEquals,

    #[token("++")]
    PlusPlus,
    #[token("--")]
    MinusMinus,
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

    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("!!")]
    NotNullAssert,
    #[token("!")]
    Bang,
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
    #[token("#")]
    Hash,

    #[regex(r"`([^`\\\r\n]|\\.)*`")]
    BacktickIdentifier,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Identifier,

    #[regex(r".", priority = 0)]
    Unknown,
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use logos::Logos;
    use tokio::sync::oneshot::error::TryRecvError;

    use crate::syntax::kotlin::models::KotlinToken;

    #[tokio::test]
    async fn test_kotlin_gradle_file() {
        let file_path =
            "/home/sanae/CodeProject/cppPro/study/taught/thread-gradle/app/build.gradle.kts";
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
                    lexer = KotlinToken::lexer(&content);
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
