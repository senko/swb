//! Line and column numbers for error messages and stack traces (ADR 0026
//! section 3): a line-start index, built on demand from the source.
//!
//! Line terminators are those of §12.3: LF, CR, CR LF (one terminator),
//! LS and PS. Columns count UTF-16 code units, as source offsets do.

use swb_js_text::Str16;

/// A line and column, both starting at 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    /// The line number.
    pub line: u32,
    /// The column number in code units.
    pub column: u32,
}

/// The offsets where lines start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    /// Builds the index of a source.
    pub fn new(source: Str16<'_>) -> Self {
        let mut starts = vec![0];
        let mut units = source.units().enumerate().peekable();
        while let Some((index, unit)) = units.next() {
            let terminator = match unit {
                0x0D => {
                    if units.peek().is_some_and(|&(_, next)| next == 0x0A) {
                        units.next();
                        index + 2
                    } else {
                        index + 1
                    }
                }
                0x0A | 0x2028 | 0x2029 => index + 1,
                _ => continue,
            };
            starts.push(terminator as u32);
        }
        LineIndex { starts }
    }

    /// The number of lines.
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// The location of a code-unit offset. An offset inside a CR LF pair
    /// belongs to the line that the pair ends.
    pub fn location(&self, offset: u32) -> Location {
        let line = self.starts.partition_point(|&start| start <= offset);
        let start = line
            .checked_sub(1)
            .and_then(|i| self.starts.get(i))
            .copied()
            .unwrap_or(0);
        Location {
            line: line.max(1) as u32,
            column: offset - start + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(source: &str) -> LineIndex {
        let units: Vec<u16> = source.encode_utf16().collect();
        LineIndex::new(Str16::Wide(&units))
    }

    #[test]
    fn lines_split_at_all_terminators() {
        let lines = index("a\nbc\r\nd\re\u{2028}f\u{2029}g");
        assert_eq!(lines.line_count(), 6);
        assert_eq!(lines.location(0), Location { line: 1, column: 1 });
        assert_eq!(lines.location(1), Location { line: 1, column: 2 });
        assert_eq!(lines.location(2), Location { line: 2, column: 1 });
        assert_eq!(lines.location(3), Location { line: 2, column: 2 });
        assert_eq!(lines.location(6), Location { line: 3, column: 1 });
        assert_eq!(lines.location(8), Location { line: 4, column: 1 });
        assert_eq!(lines.location(12), Location { line: 6, column: 1 });
    }

    #[test]
    fn columns_count_code_units() {
        // An astral character is two code units.
        let lines = index("\u{1F600}x");
        assert_eq!(lines.location(2), Location { line: 1, column: 3 });
        let narrow = LineIndex::new(Str16::Latin1(b"ab\ncd"));
        assert_eq!(narrow.location(4), Location { line: 2, column: 2 });
        // Past the end: the last line.
        assert_eq!(narrow.location(100).line, 2);
    }
}
