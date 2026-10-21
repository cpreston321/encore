//! Single-line text editing: a cursor, a selection, word-wise movement and deletion, the
//! clipboard, IME composition (marked text), and drawing the text with its caret. Positions
//! are byte offsets on char boundaries; the `*_utf16` methods speak the platform's UTF-16
//! offsets.
//!
//! The field is plain state the view owns. To edit it:
//!
//! 1. Implement [`TextFieldHost`] on the view, mapping a field id to the field.
//! 2. Draw it with [`TextInput`] inside a focusable element (for example [`text_input_box`]).
//!    While focused, it registers the platform input handler, so typing, IME composition,
//!    dead keys, press-and-hold accents, dictation and the emoji picker all arrive through
//!    [`TextFieldHost::text_field_changed`].
//! 3. Route key-downs from that element to [`TextField::handle_key`] after the view's own keys
//!    (Escape, Enter) for navigation, deletion and ⌘ shortcuts. It never inserts typed
//!    characters, and its caller must not stop propagation for keys it returns false for, or
//!    the input handler won't see them.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, Div, Element, ElementId, Entity, FocusHandle, Font,
    GlobalElementId, InputHandler, InspectorElementId, InteractiveElement, IntoElement, Keystroke, LayoutId,
    ParentElement, Pixels, Point, ShapedLine, SharedString, Stateful, Style, Styled, TextRun, UTF16Selection,
    UnderlineStyle, Window, div, fill, point, prelude::FluentBuilder, px, relative, size, transparent_black,
};

use crate::{icons::icon, theme::Theme};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextField {
    text: String,
    cursor: usize,
    /// The other end of the selection, when there is one.
    anchor: Option<usize>,
    /// Text an input method is still composing.
    marked: Option<Range<usize>>,
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
        self.marked = None;
    }

    /// The selected byte range, if any text is selected.
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    /// The byte range an input method is composing, drawn underlined.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
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
        self.marked = None;
        self.delete_selection();
        let clean = flatten(s);
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

/// Control characters become spaces. Each maps to a single UTF-16 unit either way, so
/// UTF-16 offsets into the text survive.
fn flatten(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        units += c.len_utf16();
        if units > offset {
            return i;
        }
    }
    text.len()
}

fn byte_to_utf16(text: &str, offset: usize) -> usize {
    text.char_indices().take_while(|&(i, _)| i < offset).map(|(_, c)| c.len_utf16()).sum()
}

/// Conversions to and from the UTF-16 offsets the platform's input methods use, and the
/// edits they make. An offset inside a surrogate pair snaps to the start of its character.
impl TextField {
    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        byte_to_utf16(&self.text, offset)
    }

    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        utf16_to_byte(&self.text, offset)
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range.start);
        start..self.offset_from_utf16(range.end).max(start)
    }

    pub fn selected_range_utf16(&self) -> UTF16Selection {
        let anchor = self.anchor.unwrap_or(self.cursor);
        UTF16Selection {
            range: self.range_to_utf16(&(anchor.min(self.cursor)..anchor.max(self.cursor))),
            reversed: self.cursor < anchor,
        }
    }

    pub fn marked_range_utf16(&self) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.range_to_utf16(r))
    }

    /// The text in a UTF-16 range, setting `adjusted` to the range actually returned.
    pub fn text_for_range_utf16(&self, range: Range<usize>, adjusted: &mut Option<Range<usize>>) -> String {
        let range = self.range_from_utf16(&range);
        *adjusted = Some(self.range_to_utf16(&range));
        self.text[range].to_string()
    }

    /// The range an input method edits when it doesn't name one: what it's composing, else
    /// the selection, else the cursor.
    fn edit_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone())
            .or_else(|| self.selection())
            .unwrap_or(self.cursor..self.cursor)
    }

    /// Commits `text` over `range_utf16` (see [`Self::edit_range`]), ending any composition.
    pub fn replace_text_in_range_utf16(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        if !text.is_empty() && text.chars().all(char::is_control) {
            return;
        }
        let range = self.edit_range(range_utf16);
        let text = flatten(text);
        self.text.replace_range(range.clone(), &text);
        self.cursor = range.start + text.len();
        self.anchor = None;
        self.marked = None;
    }

    /// Replaces `range_utf16` with composing text, marked until it's committed or unmarked.
    /// `selected_utf16` is relative to `text`; without it the cursor goes after the text.
    pub fn replace_and_mark_text_in_range_utf16(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
    ) {
        let range = self.edit_range(range_utf16);
        let text = flatten(text);
        self.text.replace_range(range.clone(), &text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let (start, end) = match selected_utf16 {
            Some(sel) => (utf16_to_byte(&text, sel.start), utf16_to_byte(&text, sel.end.max(sel.start))),
            None => (text.len(), text.len()),
        };
        self.cursor = range.start + end;
        self.anchor = (start != end).then_some(range.start + start);
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }
}

impl TextField {
    /// Applies the standard macOS editing keys: ⌫ / ⌥⌫ / ⌘⌫, forward delete, ← → with ⌥
    /// (word) and ⌘ (line), ⇧ to select, Home/End, ⌘A, ⌘C / ⌘X / ⌘V, and ⌃⌘Space for the
    /// emoji picker. Returns false for typed characters, which come through the input
    /// handler [`TextInput`] registers, and for keys it leaves to the caller, such as Escape,
    /// Enter, ↑ ↓ and other ⌘ shortcuts.
    pub fn handle_key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut App) -> bool {
        let (cmd, alt, shift) = (k.modifiers.platform, k.modifiers.alt, k.modifiers.shift);
        match (k.key.as_str(), cmd, alt) {
            ("space", true, _) if k.modifiers.control => window.show_character_palette(),
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
            _ => return false,
        }
        self.marked = None;
        true
    }
}

/// A view that owns [`TextField`]s and lets [`TextInput`] edit them through the platform's
/// input handler. `Field` names one of its fields, so one view can own several.
pub trait TextFieldHost: 'static + Sized {
    type Field: Copy + 'static;

    /// Runs `edit` on the field, or returns `None` if it no longer exists.
    fn text_field<R>(&mut self, field: Self::Field, edit: impl FnOnce(&mut TextField) -> R) -> Option<R>;

    /// After the input handler changes the field's text or ends a composition. The view has
    /// already been notified.
    fn text_field_changed(&mut self, _field: Self::Field, _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// Draws a [`TextField`] on one line: the text, a caret or the selection, composing text
/// underlined, and `placeholder` when empty. While focused it's the window's input handler, so
/// text input reaches the field. It fills its parent's width, is one line tall and scrolls to
/// keep the caret in view.
pub struct TextInput<V: TextFieldHost> {
    view: Entity<V>,
    field: V::Field,
    focus: FocusHandle,
    theme: Theme,
    placeholder: SharedString,
    focused: Option<bool>,
}

impl<V: TextFieldHost> TextInput<V> {
    pub fn new(view: Entity<V>, field: V::Field, focus: &FocusHandle, theme: &Theme) -> Self {
        Self { view, field, focus: focus.clone(), theme: *theme, placeholder: SharedString::default(), focused: None }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Whether it draws as focused and takes text input, for a field that shares its focus
    /// handle with another. Defaults to whether `focus` is focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }
}

pub struct TextInputLayout {
    line: ShapedLine,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    scroll: Pixels,
    focused: bool,
    placeholder: bool,
    caret: Option<Bounds<Pixels>>,
    selection: Option<Bounds<Pixels>>,
}

impl<V: TextFieldHost> IntoElement for TextInput<V> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<V: TextFieldHost> Element for TextInput<V> {
    type RequestLayoutState = ();
    type PrepaintState = TextInputLayout;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> TextInputLayout {
        let field = self.view.update(cx, |v, _| v.text_field(self.field, |f| f.clone())).unwrap_or_default();
        let focused = self.focused.unwrap_or_else(|| self.focus.is_focused(window));
        let t = &self.theme;
        let style = window.text_style();
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let caret_h = (font_size + px(3.)).min(bounds.size.height);
        let caret_top = bounds.top() + (bounds.size.height - caret_h) / 2.;

        let placeholder = field.is_empty() && !self.placeholder.is_empty();
        let (text, color) = if placeholder {
            (self.placeholder.clone(), t.text_tertiary)
        } else {
            (SharedString::from(field.text().to_string()), style.color)
        };
        let run = |len| TextRun {
            len,
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
            letter_spacing: style.letter_spacing,
        };
        let runs: Vec<TextRun> = match field.marked_range() {
            Some(m) if !placeholder => [
                run(m.start),
                TextRun {
                    underline: Some(UnderlineStyle { color: Some(color), thickness: px(1.), wavy: false }),
                    ..run(m.len())
                },
                run(text.len() - m.end),
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            _ => vec![run(text.len())],
        };
        let line = window.text_system().shape_line(text, font_size, &runs, None);

        // Past the end of the box, scroll so the caret stays in view.
        let caret_w = px(1.5);
        let x_at = |i: usize| if placeholder { px(0.) } else { line.x_for_index(i) };
        let scroll = (x_at(field.cursor()) + caret_w - bounds.size.width).max(px(0.));
        let left = bounds.left() - scroll;
        let selection = field.selection().map(|sel| {
            Bounds::from_corners(
                point(left + x_at(sel.start), caret_top),
                point(left + x_at(sel.end), caret_top + caret_h),
            )
        });
        let caret = (focused && selection.is_none())
            .then(|| Bounds::new(point(left + x_at(field.cursor()), caret_top), size(caret_w, caret_h)));
        TextInputLayout { line, font, font_size, line_height, scroll, focused, placeholder, caret, selection }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        layout: &mut TextInputLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        if layout.focused {
            let handler = FieldInputHandler {
                view: self.view.clone(),
                field: self.field,
                bounds,
                text_left: bounds.left() - layout.scroll,
                font: layout.font.clone(),
                font_size: layout.font_size,
            };
            window.handle_input(&self.focus, handler, cx);
        }
        let t = &self.theme;
        window.with_content_mask(Some(ContentMask::new(bounds)), |window| {
            if let Some(sel) = layout.selection {
                let highlight = if layout.focused { t.accent.opacity(0.38) } else { t.selected };
                window.paint_quad(fill(sel, highlight).corner_radii(px(3.)));
            }
            let nudge = if layout.placeholder && layout.caret.is_some() { px(2.) } else { px(0.) };
            let _ = layout.line.paint(
                point(bounds.left() - layout.scroll + nudge, bounds.top()),
                layout.line_height,
                window,
                cx,
            );
            if let Some(caret) = layout.caret {
                window.paint_quad(fill(caret, t.accent));
            }
        });
    }
}

/// The platform's view of a [`TextField`] for one frame, converting between its UTF-16
/// offsets and the field's.
struct FieldInputHandler<V: TextFieldHost> {
    view: Entity<V>,
    field: V::Field,
    bounds: Bounds<Pixels>,
    text_left: Pixels,
    font: Font,
    font_size: Pixels,
}

impl<V: TextFieldHost> FieldInputHandler<V> {
    fn read<R>(&self, cx: &mut App, f: impl FnOnce(&TextField) -> R) -> Option<R> {
        self.view.update(cx, |v, _| v.text_field(self.field, |field| f(field)))
    }

    fn edit(&self, window: &mut Window, cx: &mut App, f: impl FnOnce(&mut TextField)) {
        let field = self.field;
        self.view.update(cx, |v, cx| {
            let changed = v.text_field(field, |tf| {
                let (text, composing) = (tf.text.clone(), tf.is_composing());
                f(tf);
                tf.text != text || (composing && !tf.is_composing())
            });
            cx.notify();
            if changed == Some(true) {
                v.text_field_changed(field, window, cx);
            }
        });
    }

    /// The field's current text shaped as drawn, which may be newer than the last frame.
    fn shape(&self, window: &Window, text: String) -> ShapedLine {
        let run = TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
            letter_spacing: gpui::Pixels::ZERO,
        };
        window.text_system().shape_line(text.into(), self.font_size, &[run], None)
    }
}

impl<V: TextFieldHost> InputHandler for FieldInputHandler<V> {
    fn selected_text_range(&mut self, _: bool, _: &mut Window, cx: &mut App) -> Option<UTF16Selection> {
        self.read(cx, TextField::selected_range_utf16)
    }

    fn marked_text_range(&mut self, _: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.read(cx, TextField::marked_range_utf16).flatten()
    }

    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        self.read(cx, |f| f.text_for_range_utf16(range_utf16, adjusted_range))
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.edit(window, cx, |f| f.replace_text_in_range_utf16(range_utf16, text));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.edit(window, cx, |f| f.replace_and_mark_text_in_range_utf16(range_utf16, new_text, new_selected_range));
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.edit(window, cx, TextField::unmark);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let (text, range) = self.read(cx, |f| (f.text().to_string(), f.range_from_utf16(&range_utf16)))?;
        let line = self.shape(window, text);
        Some(Bounds::from_corners(
            point(self.text_left + line.x_for_index(range.start), self.bounds.top()),
            point(self.text_left + line.x_for_index(range.end), self.bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, window: &mut Window, cx: &mut App) -> Option<usize> {
        if !self.bounds.contains(&point) {
            return None;
        }
        let text = self.read(cx, |f| f.text().to_string())?;
        let index = self.shape(window, text.clone()).closest_index_for_x(point.x - self.text_left);
        Some(byte_to_utf16(&text, index))
    }
}

/// A rounded input box with an optional leading icon and an accent ring while focused. Add
/// `.track_focus(..)`, a key handler and a [`TextInput`] as its child.
pub fn text_input_box(
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

    // "é" is 2 bytes and 1 UTF-16 unit, "你" 3 and 1, "🎵" 4 and 2.
    #[test]
    fn utf16_offsets_round_trip() {
        let f = field("aé你🎵b");
        let pairs = [(0, 0), (1, 1), (3, 2), (6, 3), (10, 5), (11, 6)];
        for (byte, unit) in pairs {
            assert_eq!(f.offset_to_utf16(byte), unit, "byte {byte}");
            assert_eq!(f.offset_from_utf16(unit), byte, "unit {unit}");
        }
        assert_eq!(f.offset_from_utf16(4), 6, "inside the surrogate pair snaps to its start");
        assert_eq!(f.offset_from_utf16(99), f.text().len());
        assert_eq!(f.range_from_utf16(&(2..5)), 3..10);
        assert_eq!(f.range_to_utf16(&(3..10)), 2..5);
        let mut adjusted = None;
        assert_eq!(f.text_for_range_utf16(3..99, &mut adjusted), "🎵b");
        assert_eq!(adjusted, Some(3..6));
    }

    #[test]
    fn selection_in_utf16() {
        let mut f = field("🎵 ab");
        f.motion(true, TextField::home);
        let sel = f.selected_range_utf16();
        assert_eq!((sel.range, sel.reversed), (0..5, true));
        f.end();
        assert_eq!(f.selected_range_utf16().range, 5..5);
    }

    #[test]
    fn composition_marks_then_commits() {
        let mut f = field("say ");
        f.replace_and_mark_text_in_range_utf16(None, "n", None);
        f.replace_and_mark_text_in_range_utf16(None, "ni", None);
        assert_eq!(f.text(), "say ni");
        assert_eq!(f.marked_range(), Some(4..6));
        assert_eq!(f.marked_range_utf16(), Some(4..6));
        f.replace_and_mark_text_in_range_utf16(None, "你", Some(1..1));
        assert_eq!(f.text(), "say 你");
        assert_eq!(f.marked_range(), Some(4..7));
        assert_eq!(f.cursor(), 7);
        f.replace_text_in_range_utf16(None, "你好");
        assert_eq!(f.text(), "say 你好");
        assert!(!f.is_composing());
        assert_eq!(f.cursor(), f.text().len());
    }

    #[test]
    fn composition_selects_inside_marked_text() {
        let mut f = field("x");
        f.replace_and_mark_text_in_range_utf16(None, "かんじ", Some(1..3));
        assert_eq!(f.marked_range(), Some(1..10));
        assert_eq!(f.selected_text(), Some("んじ"));
        assert_eq!(f.selected_range_utf16().range, 2..4);
        f.replace_and_mark_text_in_range_utf16(None, "", None);
        assert_eq!(f.text(), "x");
        assert!(!f.is_composing());
    }

    #[test]
    fn dead_key_replaces_its_mark() {
        let mut f = field("caf");
        f.replace_and_mark_text_in_range_utf16(None, "´", Some(1..1));
        assert_eq!(f.text(), "caf´");
        f.replace_text_in_range_utf16(None, "é");
        assert_eq!(f.text(), "café");
        assert_eq!(f.marked_range(), None);
    }

    #[test]
    fn explicit_ranges_replace_selection_and_unmark_keeps_text() {
        let mut f = field("a🎵c");
        f.select_all();
        f.replace_text_in_range_utf16(Some(1..3), "b");
        assert_eq!(f.text(), "abc");
        assert_eq!(f.cursor(), 2);
        assert_eq!(f.selection(), None);
        f.replace_and_mark_text_in_range_utf16(Some(3..3), "d", None);
        f.unmark();
        assert_eq!(f.text(), "abcd");
        assert!(!f.is_composing());
    }

    #[test]
    fn typing_replaces_the_selection_and_ignores_control_input() {
        let mut f = field("old");
        f.select_all();
        f.replace_text_in_range_utf16(None, "n");
        assert_eq!(f.text(), "n");
        f.replace_text_in_range_utf16(None, "\n");
        f.replace_text_in_range_utf16(None, "\t");
        assert_eq!(f.text(), "n");
        f.replace_text_in_range_utf16(None, "a\tb");
        assert_eq!(f.text(), "na b");
    }
}
