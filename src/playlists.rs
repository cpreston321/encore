//! Adding songs to playlists and creating playlists (Apple's API can't remove or reorder).

use std::rc::Rc;

use gpui::{AnyElement, Context, FontWeight, Keystroke, MouseButton, Pixels, Point, SharedString, Window, div, prelude::*, px};

use crate::{
    app::{Account, ContextMenu, Load, MenuEntry, MusicApp},
    model::Song,
    textfield::TextField,
    theme::Theme,
};

/// The "New Playlist" sheet: its name field, and the song to start it with.
pub struct NewPlaylist {
    pub name: TextField,
    pub song: Option<Song>,
}

impl MusicApp {
    /// Second-level menu listing the playlists songs can go into.
    pub(crate) fn playlist_picker(&self, songs: &[Song], position: Point<Pixels>) -> ContextMenu {
        let first = songs.first().cloned();
        let mut entries = vec![Some(MenuEntry {
            label: "New Playlist…".into(),
            icon: "plus",
            action: Rc::new(move |this, window, cx| this.open_new_playlist(first.clone(), window, cx)),
        })];
        let editable: Vec<_> = self.lib_playlists().into_iter().filter(|p| p.editable).collect();
        if !editable.is_empty() {
            entries.push(None);
        }
        for p in editable.into_iter().take(30) {
            let (s, p2) = (songs.to_vec(), p.clone());
            entries.push(Some(MenuEntry {
                label: SharedString::from(p.name.clone()),
                icon: "playlist",
                action: Rc::new(move |this, _, cx| this.add_songs_to_playlist(&s, &p2, cx)),
            }));
        }
        ContextMenu { position, entries }
    }

    pub(crate) fn add_songs_to_playlist(&mut self, songs: &[Song], playlist: &crate::model::Playlist, cx: &mut Context<Self>) {
        let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        let (pid, label, key, added) = (playlist.id.clone(), playlist.name.clone(), playlist.id.clone(), songs.to_vec());
        self.fetch(cx, move |c| c.add_to_playlist(&pid, &ids), move |this, load, cx| match load {
            Load::Ready(()) => {
                let n = added.len();
                // Show them immediately; Apple's copy refreshes on the next library load.
                if let Some(Load::Ready(Some(tracks))) = this.playlist_tracks.get_mut(&key) {
                    tracks.extend(added);
                }
                let what = if n == 1 { String::new() } else { format!("{n} songs ") };
                this.show_notice(format!("Added {what}to “{label}”"), cx);
            }
            Load::Failed(e) => this.show_notice(format!("Couldn't add to “{label}”: {e}"), cx),
            Load::Loading => {}
        });
    }

    pub(crate) fn open_new_playlist(&mut self, song: Option<Song>, window: &mut Window, cx: &mut Context<Self>) {
        if self.account != Account::SignedIn {
            return self.show_notice("Sign in to Apple Music to create playlists.".into(), cx);
        }
        let mut name = TextField::default();
        name.set(song.as_ref().map_or("New Playlist".to_string(), |s| format!("{} Mix", s.title)));
        name.select_all();
        self.new_playlist = Some(NewPlaylist { name, song });
        self.menu = None;
        window.focus(&self.new_playlist_focus);
        cx.notify();
    }

    fn create_new_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sheet) = self.new_playlist.take() else { return };
        window.focus(&self.focus);
        let name = sheet.name.text().trim().to_string();
        if name.is_empty() {
            return cx.notify();
        }
        let song_id = sheet.song.map(|s| s.id);
        self.fetch(cx, move |c| c.create_playlist(&name, song_id.as_deref()), |this, load, cx| match load {
            Load::Ready(p) => {
                let label = p.name.clone();
                if let Load::Ready(lib) = &mut this.remote {
                    lib.playlists.insert(0, p);
                }
                this.show_notice(format!("Created “{label}”"), cx);
            }
            Load::Failed(e) => this.show_notice(format!("Couldn't create the playlist: {e}"), cx),
            Load::Loading => {}
        });
        cx.notify();
    }

    /// Keys for the New Playlist name field. Returns true if used.
    pub(crate) fn new_playlist_key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        use crate::textfield::TextField as F;
        let Some(sheet) = self.new_playlist.as_mut() else { return false };
        let f = &mut sheet.name;
        let (cmd, alt, shift) = (k.modifiers.platform, k.modifiers.alt, k.modifiers.shift);
        match (k.key.as_str(), cmd, alt) {
            ("enter", ..) => {
                self.create_new_playlist(window, cx);
                return true;
            }
            ("escape", ..) => {
                self.new_playlist = None;
                window.focus(&self.focus);
            }
            ("backspace", true, _) => f.delete_to_start(),
            ("backspace", _, true) => f.delete_word_back(),
            ("backspace", ..) => f.backspace(),
            ("left", true, _) => f.motion(shift, F::home),
            ("right", true, _) => f.motion(shift, F::end),
            ("left", _, true) => f.motion(shift, F::word_left),
            ("right", _, true) => f.motion(shift, F::word_right),
            ("left", ..) if shift => f.motion(true, F::left),
            ("right", ..) if shift => f.motion(true, F::right),
            ("left", ..) => f.collapse_or(true, F::left),
            ("right", ..) => f.collapse_or(false, F::right),
            ("a", true, _) => f.select_all(),
            ("v", true, _) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    f.insert(&text);
                }
            }
            _ if cmd || k.modifiers.control => return false,
            _ => match &k.key_char {
                Some(ch) if !ch.chars().any(char::is_control) => f.insert(ch),
                _ => return false,
            },
        }
        cx.notify();
        true
    }

    pub(crate) fn render_new_playlist(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.new_playlist.as_ref()?;
        let f = &sheet.name;
        let text = f.text();
        let field_text: AnyElement = match f.selection() {
            Some(sel) => div()
                .flex()
                .whitespace_nowrap()
                .child(SharedString::from(text[..sel.start].to_string()))
                .child(div().rounded(px(3.)).bg(t.accent.opacity(0.38)).child(SharedString::from(text[sel.clone()].to_string())))
                .child(SharedString::from(text[sel.end..].to_string()))
                .into_any_element(),
            None => {
                let (a, b) = text.split_at(f.cursor());
                div()
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .child(SharedString::from(a.to_string()))
                    .child(div().w(px(1.5)).h(px(17.)).bg(t.accent))
                    .child(SharedString::from(b.to_string()))
                    .into_any_element()
            }
        };
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_4()
                .h(px(30.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .font_weight(FontWeight::SEMIBOLD)
                .when(primary, |d| d.bg(t.accent).text_color(gpui::white()))
                .when(!primary, |d| d.bg(t.button))
                .hover(|s| s.opacity(0.85))
                .child(label)
        };
        Some(
            div()
                .id("new-playlist-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.35))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.new_playlist = None;
                    window.focus(&this.focus);
                    cx.notify();
                }))
                .child(
                    div()
                        .id("new-playlist")
                        .w(px(420.))
                        .p_6()
                        .rounded(px(16.))
                        .bg(t.menu)
                        .border_1()
                        .border_color(t.panel_border)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().text_size(px(17.)).font_weight(FontWeight::BOLD).child("New Playlist"))
                        .when_some(sheet.song.as_ref(), |d, s| {
                            d.child(div().text_color(t.text_secondary).child(format!("Starting with “{}”", s.title)))
                        })
                        .child(
                            div()
                                .id("new-playlist-field")
                                .key_context("SearchField")
                                .track_focus(&self.new_playlist_focus)
                                .h(px(36.))
                                .px_3()
                                .flex()
                                .items_center()
                                .rounded(px(9.))
                                .bg(t.field)
                                .border_2()
                                .border_color(t.accent.opacity(0.55))
                                .overflow_hidden()
                                .child(field_text),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(button("np-cancel", "Cancel", false).on_click(cx.listener(|this, _, window, cx| {
                                    this.new_playlist = None;
                                    window.focus(&this.focus);
                                    cx.notify();
                                })))
                                .child(button("np-create", "Create", true).on_click(cx.listener(|this, _, window, cx| {
                                    this.create_new_playlist(window, cx);
                                }))),
                        ),
                )
                .into_any_element(),
        )
    }
}
