pub enum GroovyToken {
    Curly(PairSide),
    Bracket(PairSide),
    Key,
    Type,
    IntLiteral,
}

pub enum PairSide {
    Left,
    Right,
}

pub enum GroovyKeyWord {
    Def,
    Var,
}
