//! An editable text with a caret and a selection: the model of text
//! fields and text areas in pages, and of the browser's address bar.
//!
//! Positions are byte offsets at character boundaries; the caret moves
//! and deletes by grapheme clusters (user-perceived characters, UAX #29).
//! The selection is the range between `anchor` and `cursor`; the caret is
//! at `cursor`. Inserted text keeps tabs and drops other control
//! characters; a single-line text turns line breaks into spaces (as
//! Chromium does when text is pasted into a text field), a multi-line
//! text keeps them as `\n`. The maximum length counts UTF-16 code units,
//! as the HTML `maxlength` attribute does.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// An editable text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    cursor: usize,
    anchor: usize,
    multiline: bool,
    max_len: Option<usize>,
}

impl TextEdit {
    /// An empty single-line text.
    pub fn new() -> Self {
        TextEdit::default()
    }

    /// An empty text that keeps line breaks.
    pub fn multiline() -> Self {
        TextEdit {
            multiline: true,
            ..TextEdit::default()
        }
    }

    /// Limits the length of typed and pasted text to `max` UTF-16 code
    /// units. Text that is already longer stays.
    pub fn set_max_len(&mut self, max: Option<usize>) {
        self.max_len = max;
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and selects all of it.
    pub fn set_text(&mut self, text: &str) {
        self.replace_text(text);
        self.select_all();
    }

    /// Replaces the text without the length limit (a value set by the
    /// page, not typed) and puts the caret at the end.
    pub fn replace_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    /// The caret position.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The selected range (may be empty).
    pub fn selection(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    /// The selected text.
    pub fn selected_text(&self) -> &str {
        self.text.get(self.selection()).unwrap_or("")
    }

    /// Selects from `anchor` to `cursor` (clamped to character
    /// boundaries).
    pub fn select(&mut self, anchor: usize, cursor: usize) {
        self.anchor = self.boundary(anchor);
        self.cursor = self.boundary(cursor);
    }

    /// Selects the whole text.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Inserts text at the caret, replacing the selection. Line breaks
    /// become `\n` in a multi-line text and spaces in a single-line text;
    /// other control characters except tabs are dropped. The text is cut to
    /// the maximum length. Returns true if the text changed.
    pub fn insert(&mut self, s: &str) -> bool {
        let line_break = if self.multiline { '\n' } else { ' ' };
        let mut clean: String = s
            .replace("\r\n", "\n")
            .chars()
            .map(|c| {
                if c == '\r' || c == '\n' {
                    line_break
                } else {
                    c
                }
            })
            .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
            .collect();
        let range = self.selection();
        if let Some(max) = self.max_len {
            let kept = utf16_len(&self.text) - utf16_len(self.selected_text());
            truncate_utf16(&mut clean, max.saturating_sub(kept));
        }
        if clean.is_empty() && range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), &clean);
        self.cursor = range.start + clean.len();
        self.anchor = self.cursor;
        true
    }

    /// Deletes the selection, or the grapheme cluster before the caret.
    /// Returns true if the text changed.
    pub fn backspace(&mut self) -> bool {
        if self.selection().is_empty()
            && let Some(prev) = self.prev_boundary(self.cursor)
        {
            self.anchor = prev;
        }
        self.delete_selection()
    }

    /// Deletes the selection, or the grapheme cluster after the caret.
    /// Returns true if the text changed.
    pub fn delete(&mut self) -> bool {
        if self.selection().is_empty()
            && let Some(next) = self.next_boundary(self.cursor)
        {
            self.anchor = next;
        }
        self.delete_selection()
    }

    /// Deletes the selected text. Returns true if there was any.
    pub fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = range.start;
        true
    }

    /// Moves the caret one grapheme cluster left; extends the selection if
    /// `select`. Without `select`, a selection collapses to its start.
    pub fn left(&mut self, select: bool) {
        let target = if !select && !self.selection().is_empty() {
            self.selection().start
        } else {
            self.prev_boundary(self.cursor).unwrap_or(self.cursor)
        };
        self.move_to(target, select);
    }

    /// Moves the caret one grapheme cluster right.
    pub fn right(&mut self, select: bool) {
        let target = if !select && !self.selection().is_empty() {
            self.selection().end
        } else {
            self.next_boundary(self.cursor).unwrap_or(self.cursor)
        };
        self.move_to(target, select);
    }

    /// Moves the caret to the start of the previous word.
    pub fn word_left(&mut self, select: bool) {
        let target = self
            .text
            .get(..self.cursor)
            .and_then(|before| {
                before
                    .split_word_bound_indices()
                    .rev()
                    .find(|(_, w)| w.chars().any(char::is_alphanumeric))
                    .map(|(i, _)| i)
            })
            .unwrap_or(0);
        self.move_to(target, select);
    }

    /// Moves the caret to the end of the next word.
    pub fn word_right(&mut self, select: bool) {
        let target = self
            .text
            .get(self.cursor..)
            .and_then(|after| {
                after
                    .split_word_bound_indices()
                    .find(|(_, w)| w.chars().any(char::is_alphanumeric))
                    .map(|(i, w)| self.cursor + i + w.len())
            })
            .unwrap_or(self.text.len());
        self.move_to(target, select);
    }

    /// Moves the caret to the start of the line (the text for a
    /// single-line text).
    pub fn home(&mut self, select: bool) {
        let start = self.line_start(self.cursor);
        self.move_to(start, select);
    }

    /// Moves the caret to the end of the line.
    pub fn end(&mut self, select: bool) {
        let end = self.line_end(self.cursor);
        self.move_to(end, select);
    }

    /// Moves the caret to the start of the text.
    pub fn text_start(&mut self, select: bool) {
        self.move_to(0, select);
    }

    /// Moves the caret to the end of the text.
    pub fn text_end(&mut self, select: bool) {
        self.move_to(self.text.len(), select);
    }

    /// Moves the caret to the same column (in characters) of the previous
    /// line, or to the start of the text on the first line. Lines are
    /// separated by `\n` (wrapped lines count as one).
    pub fn line_up(&mut self, select: bool) {
        let start = self.line_start(self.cursor);
        if start == 0 {
            self.move_to(0, select);
            return;
        }
        let column = self.text[start..self.cursor].chars().count();
        let prev_start = self.line_start(start - 1);
        let target = self.column_in_line(prev_start, column);
        self.move_to(target, select);
    }

    /// Moves the caret to the same column of the next line, or to the end
    /// of the text on the last line.
    pub fn line_down(&mut self, select: bool) {
        let end = self.line_end(self.cursor);
        if end >= self.text.len() {
            self.move_to(self.text.len(), select);
            return;
        }
        let column = self.text[self.line_start(self.cursor)..self.cursor]
            .chars()
            .count();
        let target = self.column_in_line(end + 1, column);
        self.move_to(target, select);
    }

    /// Selects the word (or run of spaces or punctuation) at `pos`.
    pub fn select_word_at(&mut self, pos: usize) {
        let found = self
            .text
            .split_word_bound_indices()
            .find(|(start, word)| pos < start + word.len())
            .or_else(|| self.text.split_word_bound_indices().next_back());
        if let Some((start, word)) = found {
            self.anchor = start;
            self.cursor = start + word.len();
        }
    }

    /// Moves the caret to `pos` (clamped to a character boundary);
    /// extends the selection if `select`.
    pub fn move_to(&mut self, pos: usize, select: bool) {
        let pos = self.boundary(pos);
        self.cursor = pos;
        if !select {
            self.anchor = pos;
        }
    }

    /// `pos` clamped to the text and moved back to a character boundary.
    fn boundary(&self, pos: usize) -> usize {
        self.text.floor_char_boundary(pos.min(self.text.len()))
    }

    /// The start of the grapheme cluster before `pos`.
    fn prev_boundary(&self, pos: usize) -> Option<usize> {
        let before = self.text.get(..pos)?;
        before.graphemes(true).next_back().map(|g| pos - g.len())
    }

    /// The end of the grapheme cluster after `pos`.
    fn next_boundary(&self, pos: usize) -> Option<usize> {
        let after = self.text.get(pos..)?;
        after.graphemes(true).next().map(|g| pos + g.len())
    }

    fn line_start(&self, pos: usize) -> usize {
        if !self.multiline {
            return 0;
        }
        self.text
            .get(..pos)
            .and_then(|before| before.rfind('\n'))
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self, pos: usize) -> usize {
        if !self.multiline {
            return self.text.len();
        }
        self.text
            .get(pos..)
            .and_then(|after| after.find('\n'))
            .map_or(self.text.len(), |i| pos + i)
    }

    /// The position `column` characters into the line that starts at
    /// `start`, or its end.
    fn column_in_line(&self, start: usize, column: usize) -> usize {
        let end = self.line_end(start);
        self.text[start..end]
            .char_indices()
            .nth(column)
            .map_or(end, |(i, _)| start + i)
    }
}

/// The length of `s` in UTF-16 code units.
fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Cuts `s` to at most `max` UTF-16 code units, at a character boundary.
fn truncate_utf16(s: &mut String, max: usize) {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > max {
            s.truncate(i);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_replaces_selection() {
        let mut f = TextEdit::new();
        f.set_text("https://old/");
        f.insert("new.example");
        assert_eq!(f.text(), "new.example");
        assert_eq!(f.cursor(), 11);
    }

    #[test]
    fn editing_and_movement() {
        let mut f = TextEdit::new();
        f.insert("abč");
        f.left(false);
        f.backspace();
        assert_eq!(f.text(), "ač");
        f.end(false);
        f.left(true);
        assert_eq!(f.selection(), 1..3);
        f.delete();
        assert_eq!(f.text(), "a");
        f.home(false);
        f.delete();
        assert_eq!(f.text(), "");
        assert!(!f.backspace());
        assert_eq!(f.text(), "");
    }

    #[test]
    fn line_breaks_and_control_characters() {
        let mut f = TextEdit::new();
        f.insert("a\nb\r\nc\td\u{7}");
        assert_eq!(f.text(), "a b c\td");
        let mut m = TextEdit::multiline();
        m.insert("a\r\nb\rc\u{0}");
        assert_eq!(m.text(), "a\nb\nc");
    }

    #[test]
    fn the_caret_moves_by_grapheme_clusters() {
        let mut f = TextEdit::new();
        f.insert("a\u{1F44D}\u{1F3FD}e\u{301}b");
        f.left(false);
        f.left(false);
        assert_eq!(&f.text()[f.cursor()..], "e\u{301}b");
        f.left(false);
        assert_eq!(f.cursor(), 1);
        f.delete();
        assert_eq!(f.text(), "ae\u{301}b");
        f.right(false);
        f.backspace();
        assert_eq!(f.text(), "ab");
    }

    #[test]
    fn maximum_length_counts_utf16_units() {
        let mut f = TextEdit::new();
        f.set_max_len(Some(3));
        f.insert("ab😀c");
        // The emoji is two UTF-16 units: it does not fit after "ab".
        assert_eq!(f.text(), "ab");
        f.select_all();
        f.insert("😀xyz");
        assert_eq!(f.text(), "😀x");
        assert!(!f.insert("q"));
    }

    #[test]
    fn words_and_lines() {
        let mut f = TextEdit::multiline();
        f.insert("one two\nthree four");
        f.word_left(false);
        assert_eq!(&f.text()[f.cursor()..], "four");
        f.word_left(true);
        assert_eq!(f.selected_text(), "three ");
        f.move_to(11, false);
        f.line_up(false);
        assert_eq!(f.cursor(), 3);
        f.line_down(false);
        assert_eq!(f.cursor(), 11);
        f.home(false);
        assert_eq!(f.cursor(), 8);
        f.word_right(false);
        assert_eq!(f.cursor(), 13);
        f.select_word_at(1);
        assert_eq!(f.selected_text(), "one");
        f.text_start(false);
        f.line_up(false);
        assert_eq!(f.cursor(), 0);
        f.text_end(false);
        f.line_down(false);
        assert_eq!(f.cursor(), f.text().len());
    }
}
