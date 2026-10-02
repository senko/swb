//! An editable single-line text: the model behind the address bar.
//!
//! Positions are byte offsets at character boundaries. The selection is the
//! range between `anchor` and `cursor`.

/// A single-line text editing model.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextField {
    text: String,
    cursor: usize,
    anchor: usize,
}

impl TextField {
    /// The text.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and selects all of it.
    pub(crate) fn set_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
        self.select_all();
    }

    /// The cursor position.
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// The selected range (may be empty).
    pub(crate) fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    /// Selects the whole text.
    pub(crate) fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Inserts text at the cursor, replacing the selection.
    pub(crate) fn insert(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let range = self.selection();
        self.text.replace_range(range.clone(), &s);
        self.cursor = range.start + s.len();
        self.anchor = self.cursor;
    }

    /// Deletes the selection, or the character before the cursor.
    pub(crate) fn backspace(&mut self) {
        if self.selection().is_empty()
            && let Some(prev) = self.prev_boundary(self.cursor)
        {
            self.anchor = prev;
        }
        self.insert("");
    }

    /// Deletes the selection, or the character after the cursor.
    pub(crate) fn delete(&mut self) {
        if self.selection().is_empty()
            && let Some(next) = self.next_boundary(self.cursor)
        {
            self.anchor = next;
        }
        self.insert("");
    }

    /// Moves the cursor one character left; extends the selection if
    /// `select`.
    pub(crate) fn left(&mut self, select: bool) {
        let target = if !select && !self.selection().is_empty() {
            self.selection().start
        } else {
            self.prev_boundary(self.cursor).unwrap_or(self.cursor)
        };
        self.move_to(target, select);
    }

    /// Moves the cursor one character right.
    pub(crate) fn right(&mut self, select: bool) {
        let target = if !select && !self.selection().is_empty() {
            self.selection().end
        } else {
            self.next_boundary(self.cursor).unwrap_or(self.cursor)
        };
        self.move_to(target, select);
    }

    /// Moves the cursor to the start.
    pub(crate) fn home(&mut self, select: bool) {
        self.move_to(0, select);
    }

    /// Moves the cursor to the end.
    pub(crate) fn end(&mut self, select: bool) {
        self.move_to(self.text.len(), select);
    }

    /// Moves the cursor to `pos` (clamped to a character boundary).
    pub(crate) fn move_to(&mut self, pos: usize, select: bool) {
        let mut pos = pos.min(self.text.len());
        while !self.text.is_char_boundary(pos) {
            pos -= 1;
        }
        self.cursor = pos;
        if !select {
            self.anchor = pos;
        }
    }

    fn prev_boundary(&self, pos: usize) -> Option<usize> {
        self.text[..pos].char_indices().next_back().map(|(i, _)| i)
    }

    fn next_boundary(&self, pos: usize) -> Option<usize> {
        self.text[pos..].chars().next().map(|c| pos + c.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_replaces_selection() {
        let mut f = TextField::default();
        f.set_text("https://old/");
        f.insert("new.example");
        assert_eq!(f.text(), "new.example");
        assert_eq!(f.cursor(), 11);
    }

    #[test]
    fn editing_and_movement() {
        let mut f = TextField::default();
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
        f.backspace();
        assert_eq!(f.text(), "");
    }

    #[test]
    fn control_characters_are_dropped() {
        let mut f = TextField::default();
        f.insert("a\nb\tc");
        assert_eq!(f.text(), "abc");
    }
}
