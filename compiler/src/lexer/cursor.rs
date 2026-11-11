//! A position in the source text that moves forward one character at a time.

pub(super) struct Cursor<'src> {
    text: &'src str,
    offset: usize,
}

impl<'src> Cursor<'src> {
    pub fn new(text: &'src str) -> Self {
        Self { text, offset: 0 }
    }

    /// The current byte offset. `SourceFile` guarantees it fits in a `u32`.
    pub fn offset(&self) -> u32 {
        self.offset as u32
    }

    pub fn rest(&self) -> &'src str {
        self.text.get(self.offset..).unwrap_or_default()
    }

    /// The text from `start` up to the cursor.
    pub fn slice_from(&self, start: u32) -> &'src str {
        self.text
            .get(start as usize..self.offset)
            .unwrap_or_default()
    }

    pub fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub fn character_before(&self, offset: u32) -> Option<char> {
        self.text.get(..offset as usize)?.chars().next_back()
    }

    pub fn starts_with(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    pub fn is_at_end(&self) -> bool {
        self.offset >= self.text.len()
    }

    /// True at a line break or at the end of the file.
    pub fn is_at_line_end(&self) -> bool {
        self.is_at_end() || self.starts_with("\n") || self.starts_with("\r\n")
    }

    pub fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        Some(character)
    }

    pub fn eat(&mut self, expected: char) -> bool {
        let matches = self.peek() == Some(expected);
        if matches {
            self.offset += expected.len_utf8();
        }
        matches
    }

    pub fn eat_str(&mut self, expected: &str) -> bool {
        let matches = self.starts_with(expected);
        if matches {
            self.offset += expected.len();
        }
        matches
    }

    pub fn eat_while(&mut self, predicate: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&predicate) {
            self.bump();
        }
    }

    /// Consumes `\n` or `\r\n`.
    pub fn eat_line_break(&mut self) -> bool {
        self.eat_str("\r\n") || self.eat('\n')
    }

    /// Moves to the line break that ends the current line, without consuming it.
    pub fn skip_to_line_end(&mut self) {
        while !self.is_at_line_end() {
            self.bump();
        }
    }

    /// Moves forward by a byte count measured on `rest()`, which must end on a character boundary.
    pub fn advance_bytes(&mut self, count: usize) {
        let target = (self.offset + count).min(self.text.len());
        debug_assert!(self.text.is_char_boundary(target));
        self.offset = target;
    }
}
