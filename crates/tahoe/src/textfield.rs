//! Single-line text editing: a cursor, a selection, word-wise movement and deletion, the
//! clipboard, and drawing the text with its caret. Positions are byte offsets on char
//! boundaries.
//!
//! The field is plain state the view owns. Route key-downs from the focused element to
//! [`TextField::handle_key`] after the view's own keys (Escape, Enter), and draw it with
//! [`TextField::render_text`] inside whatever box suits the view, or [`text_input`].

use std::ops::Range;

use gpui::{
    AnyElement, App, ClipboardItem, Div, ElementId, InteractiveElement, IntoElement, Keystroke, ParentElement,
    SharedString, Stateful, Styled, div, prelude::FluentBuilder, px, transparent_black,
};

use crate::{icons::icon, theme::Theme};

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

impl TextField {
    /// Applies the standard macOS editing keys: typing, ⌫ / ⌥⌫ / ⌘⌫, forward delete, ← → with
    /// ⌥ (word) and ⌘ (line), ⇧ to select, Home/End, ⌘A, and ⌘C / ⌘X / ⌘V. Returns false for
    /// keys it leaves to the caller, such as Escape, Enter, ↑ ↓ and other ⌘ shortcuts.
    pub fn handle_key(&mut self, k: &Keystroke, cx: &mut App) -> bool {
        let (cmd, alt, shift) = (k.modifiers.platform, k.modifiers.alt, k.modifiers.shift);
        match (k.key.as_str(), cmd, alt) {
            ("backspace", true, _) => self.delete_to_start(),
            ("backspace", _, true) => self.delete_word_back(),
            ("backspace", ..) => self.backspace(),
            ("delete", ..) => self.delete_forward(),
            ("left", true, _) | ("home", ..) => self.motion(shift, Self::home),
            ("right", true, _) | ("end", ..) => self.motion(shift, Self::end),
            ("left", _, true) => self.motion(shift, Self::word_left),
            ("right", _, true) => self.motion(shift, Self::word_right),
            ("left", ..) if shift => self.motion(true, Self::left),
            ("right", ..) if shift => self.motion(true, Self::right),
            ("left", ..) => self.collapse_or(true, Self::left),
            ("right", ..) => self.collapse_or(false, Self::right),
            ("a", true, _) => self.select_all(),
            ("c", true, _) | ("x", true, _) => {
                if let Some(text) = self.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
                    if k.key == "x" {
                        self.backspace();
                    }
                }
            }
            ("v", true, _) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.insert(&text);
                }
            }
            _ if cmd || k.modifiers.control => return false,
            _ => match &k.key_char {
                Some(ch) if !ch.chars().any(char::is_control) => self.insert(ch),
                _ => return false,
            },
        }
        true
    }

    /// The text with a caret at the cursor and the selection highlighted, or `placeholder`
    /// when empty. `caret_h` should match the font's line height.
    pub fn render_text(
        &self,
        t: &Theme,
        focused: bool,
        placeholder: impl Into<SharedString>,
        caret_h: f32,
    ) -> AnyElement {
        let text = self.text();
        let placeholder = placeholder.into();
        if text.is_empty() && !focused {
            return div().text_color(t.text_tertiary).child(placeholder).into_any_element();
        }
        let row = div().flex().items_center().min_w_0().overflow_hidden().whitespace_nowrap();
        let piece = |s: &str| SharedString::from(s.to_string());
        if let Some(sel) = self.selection() {
            // Like macOS: no caret while text is selected, just the highlight.
            let highlight = if focused { t.accent.opacity(0.38) } else { t.selected };
            return row
                .when(sel.start > 0, |d| d.child(piece(&text[..sel.start])))
                .child(div().rounded(px(3.)).bg(highlight).child(piece(&text[sel.clone()])))
                .when(sel.end < text.len(), |d| d.child(piece(&text[sel.end..])))
                .into_any_element();
        }
        let (before, after) = text.split_at(self.cursor());
        row.when(!before.is_empty(), |d| d.child(piece(before)))
            .when(focused, |d| d.child(div().flex_none().w(px(1.5)).h(px(caret_h)).bg(t.accent)))
            .when(!after.is_empty(), |d| d.child(piece(after)))
            .when(text.is_empty(), |d| d.child(div().pl_0p5().text_color(t.text_tertiary).child(placeholder)))
            .into_any_element()
    }
}

/// A rounded input box with an optional leading icon and an accent ring while focused. Add
/// `.track_focus(..)`, a key handler and [`TextField::render_text`] as its child.
pub fn text_input(
    t: &Theme,
    id: impl Into<ElementId>,
    leading_icon: Option<&'static str>,
    focused: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px_2p5()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(8.))
        .bg(t.field)
        .border_1()
        .border_color(if focused { t.accent.opacity(0.55) } else { transparent_black() })
        .overflow_hidden()
        .when_some(leading_icon, |d, name| d.child(icon(name, 12., t.text_secondary)))
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
