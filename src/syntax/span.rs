#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSpan {
    pub start_bytes: usize,
    pub end_bytes: usize,
    pub start_char: TextLocation,
    pub end_char: TextLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct TextLocation {
    pub col: usize,
    pub row: usize,
}

impl TextLocation {
    pub fn new(col: usize, row: usize) -> Self {
        Self { col, row }
    }
}
