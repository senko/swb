//! The positions of inline scripts in the source text of a document (M8
//! feature 5c): error reports of an inline script count lines and columns
//! from the start of the document, as Chromium does, so the runtime needs
//! the position of the script's first character.
//!
//! html5ever counts lines but not columns, and it does not report offsets.
//! At a `script` end tag the parser knows how much input is consumed (the
//! end of the end tag) and the element's text. The end tag starts at the
//! last `</script` before that point (the text of a script cannot contain
//! one that ends it), and the text, which is raw text, comes right before
//! it: in the source it differs from the element's text only by the
//! newline normalization (`\r\n` and `\r` become `\n`), so walking back as
//! many characters as the text has gives its start. A cursor keeps the line
//! and the UTF-16 column, so the whole parse counts each character once.
//! Only `\n` ends a line (`\r\n` too, by its `\n`); a lone `\r` does not,
//! as in Chromium (measured).

/// A position in the source text of a document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextPosition {
    /// The line, from 0.
    pub line: u32,
    /// The column in UTF-16 code units, from 0.
    pub column: u32,
}

/// The source text and the line count up to a cursor.
pub(super) struct Positions {
    source: String,
    /// A byte offset at a character boundary; the next script starts after
    /// it.
    cursor: usize,
    /// The line of the cursor (from 0).
    line: u32,
    /// The column of the cursor in UTF-16 code units.
    column: u32,
}

impl Positions {
    pub(super) fn new(source: &str) -> Positions {
        Positions {
            source: source.to_owned(),
            cursor: 0,
            line: 0,
            column: 0,
        }
    }

    /// The length of the source text in bytes.
    pub(super) fn len(&self) -> usize {
        self.source.len()
    }

    /// The position of the first character of the text of a script whose
    /// end tag ends at byte `end`, with `text_chars` characters of text.
    pub(super) fn script_start(&mut self, end: usize, text_chars: usize) -> TextPosition {
        let end = end.min(self.source.len());
        let tag = self.end_tag_start(end).unwrap_or(end);
        let start = self.walk_back(tag, text_chars);
        self.advance_to(start);
        TextPosition {
            line: self.line,
            column: self.column,
        }
    }

    /// The byte offset of the last `</script` (any case) between the cursor
    /// and `end`.
    fn end_tag_start(&self, end: usize) -> Option<usize> {
        const TAG: &[u8] = b"</script";
        let bytes = self.source.as_bytes();
        let window = bytes.get(self.cursor..end)?;
        let found = window
            .windows(TAG.len())
            .rposition(|w| w.eq_ignore_ascii_case(TAG))?;
        Some(self.cursor + found)
    }

    /// The offset `chars` characters before `from` (a `\r\n` pair counts as
    /// one), but not before the cursor.
    fn walk_back(&self, from: usize, chars: usize) -> usize {
        let bytes = self.source.as_bytes();
        let mut position = from;
        for _ in 0..chars {
            if position <= self.cursor {
                return self.cursor;
            }
            let Some(c) = self
                .source
                .get(..position)
                .and_then(|s| s.chars().next_back())
            else {
                return self.cursor;
            };
            position -= c.len_utf8();
            if c == '\n' && position > self.cursor && bytes.get(position - 1) == Some(&b'\r') {
                position -= 1;
            }
        }
        position
    }

    /// Moves the cursor to `to` (not backwards) and counts the lines and
    /// the column on the way: a `\n` ends a line.
    fn advance_to(&mut self, to: usize) {
        let bytes = self.source.as_bytes();
        let to = to.max(self.cursor);
        for &byte in bytes.get(self.cursor..to).unwrap_or_default() {
            match byte {
                b'\n' => {
                    self.line = self.line.saturating_add(1);
                    self.column = 0;
                }
                // A continuation byte belongs to the character before.
                0x80..=0xBF => {}
                // The lead byte of a character outside the BMP: two units.
                0xF0..=0xFF => self.column = self.column.saturating_add(2),
                _ => self.column = self.column.saturating_add(1),
            }
        }
        self.cursor = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(source: &str, end_marker: &str, text: &str) -> TextPosition {
        let end = source.find(end_marker).unwrap() + end_marker.len();
        Positions::new(source).script_start(end, text.chars().count())
    }

    #[test]
    fn finds_the_start_of_the_text() {
        let source = "<p>x</p>   <script>throw 1</script>";
        assert_eq!(
            start(source, "</script>", "throw 1"),
            TextPosition {
                line: 0,
                column: 19
            }
        );
        let source = "<!DOCTYPE html>\r\n<p>\u{e9}</p><SCRIPT>\r\na()\r\n</SCRIPT >";
        assert_eq!(
            start(source, "</SCRIPT >", "\na()\n"),
            TextPosition {
                line: 1,
                column: 16
            }
        );
        // A column counts UTF-16 units.
        let source = "\u{1F600}<script>x</script>";
        assert_eq!(start(source, "</script>", "x").column, 10);
    }

    #[test]
    fn counts_lines_across_scripts() {
        let source = "<script>a</script>\n<script>a</script>\r\n<script>\nb</script>";
        let mut positions = Positions::new(source);
        let mut ends = source.match_indices("</script>").map(|(i, s)| i + s.len());
        let first = positions.script_start(ends.next().unwrap(), 1);
        let second = positions.script_start(ends.next().unwrap(), 1);
        let third = positions.script_start(ends.next().unwrap(), 2);
        assert_eq!((first.line, first.column), (0, 8));
        assert_eq!((second.line, second.column), (1, 8));
        assert_eq!((third.line, third.column), (2, 8));
        // A lone CR is not a line break (Chromium).
        let source = "\r\r<script>a</script>\r<script>b</script>";
        let mut positions = Positions::new(source);
        let mut ends = source.match_indices("</script>").map(|(i, s)| i + s.len());
        let first = positions.script_start(ends.next().unwrap(), 1);
        let second = positions.script_start(ends.next().unwrap(), 1);
        assert_eq!((first.line, first.column), (0, 10));
        assert_eq!((second.line, second.column), (0, 29));
    }
}
