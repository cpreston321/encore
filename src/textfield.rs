//! Single-line text editing for the search field: a cursor, a selection, word-wise
//! movement and deletion, clipboard. Positions are byte offsets on char boundaries.

use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextField {
    text: String,
    cursor: usize,
    /// The other end of the selection, when there is one.
    anchor: Option<usize>,
}

impl TextField {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.anchor = None;
    }

    /// The selected byte range, if any text is selected.
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Removes the selection; true if there was one.
    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = None;
        true
    }

    /// Moves the cursor with `motion`, extending the selection when `extend` is set and
    /// otherwise collapsing it.
    pub fn motion(&mut self, extend: bool, motion: fn(&mut Self)) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
            motion(self);
        } else {
            self.anchor = None;
            motion(self);
        }
    }

    /// ← / → with a selection jump to its start / end instead of moving one character.
    pub fn collapse_or(&mut self, to_start: bool, motion: fn(&mut Self)) {
        match self.selection() {
            Some(r) => {
                self.cursor = if to_start { r.start } else { r.end };
                self.anchor = None;
            }
            None => self.motion(false, motion),
        }
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// Inserts at the cursor; newlines and tabs (e.g. from a paste) become spaces.
    pub fn insert(&mut self, s: &str) {
        self.delete_selection();
        let clean: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(prev) = self.prev_boundary(self.cursor) {
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(next) = self.next_boundary(self.cursor) {
            self.text.replace_range(self.cursor..next, "");
        }
    }

    /// ⌥⌫: back to the start of the previous word.
    pub fn delete_word_back(&mut self) {
        if self.delete_selection() {
            return;
        }
        let start = self.word_start(self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// ⌘⌫: everything before the cursor.
    pub fn delete_to_start(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor).unwrap_or(0);
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor).unwrap_or(self.text.len());
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_start(self.cursor);
    }

    pub fn word_right(&mut self) {
        let rest = &self.text[self.cursor..];
        let skip_space = rest.len() - rest.trim_start().len();
        let word = rest.trim_start().find(char::is_whitespace).unwrap_or(rest.trim_start().len());
        self.cursor += skip_space + word;
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    fn prev_boundary(&self, at: usize) -> Option<usize> {
        self.text[..at].char_indices().next_back().map(|(i, _)| i)
    }

    fn next_boundary(&self, at: usize) -> Option<usize> {
        self.text[at..].chars().next().map(|c| at + c.len_utf8())
    }

    fn word_start(&self, at: usize) -> usize {
        let before = self.text[..at].trim_end();
        before.rfind(char::is_whitespace).map_or(0, |i| i + before[i..].chars().next().map_or(1, char::len_utf8))
    }
}

#[cfg(test)]
mod tests {
    use super::TextField;

    fn field(text: &str) -> TextField {
        let mut f = TextField::default();
        f.set(text);
        f
    }

    #[test]
    fn edits_at_the_cursor() {
        let mut f = field("tayor");
        f.left();
        f.left();
        f.insert("l");
        assert_eq!(f.text(), "taylor");
        f.end();
        f.backspace();
        assert_eq!(f.text(), "taylo");
    }

    #[test]
    fn word_movement_and_deletion() {
        let mut f = field("taylor swift  live");
        f.delete_word_back();
        assert_eq!(f.text(), "taylor swift  ");
        f.delete_word_back();
        assert_eq!(f.text(), "taylor ");
        f.home();
        f.word_right();
        assert_eq!(f.cursor(), 6);
        f.end();
        f.word_left();
        assert_eq!(f.cursor(), 0);
    }

    #[test]
    fn handles_multibyte_text() {
        let mut f = field("Ádéla");
        f.backspace();
        assert_eq!(f.text(), "Ádél");
        f.home();
        f.right();
        f.delete_forward();
        assert_eq!(f.text(), "Áél");
    }

    #[test]
    fn select_all_then_type_replaces() {
        let mut f = field("old query");
        f.select_all();
        assert_eq!(f.selected_text(), Some("old query"));
        f.insert("new");
        assert_eq!(f.text(), "new");
        assert_eq!(f.selection(), None);
    }

    #[test]
    fn shift_selection_and_collapse() {
        let mut f = field("taylor swift");
        f.motion(true, TextField::word_left);
        assert_eq!(f.selected_text(), Some("swift"));
        f.collapse_or(true, TextField::left);
        assert_eq!(f.cursor(), 7);
        assert_eq!(f.selection(), None);
        f.motion(true, TextField::home);
        f.backspace();
        assert_eq!(f.text(), "swift");
    }

    #[test]
    fn paste_flattens_newlines_and_delete_to_start() {
        let mut f = field("ab");
        f.insert("c\nd");
        assert_eq!(f.text(), "abc d");
        f.left();
        f.delete_to_start();
        assert_eq!(f.text(), "d");
    }
}
