//! Connecting a MusicKit key from the welcome screen: Encore signs in to Apple Music with a
//! key from the user's Apple Developer account, so a build without one starts here.

use std::path::PathBuf;

use gpui::{App, FocusHandle, Keystroke, PathPromptOptions, Window};

use super::*;
use crate::{api::Auth, keys::TextFieldId};

const KEYS_PAGE: &str = "https://developer.apple.com/account/resources/authkeys/list";

pub(crate) struct KeySetup {
    pub team_id: tahoe::TextField,
    pub key_id: tahoe::TextField,
    pub file: Option<PathBuf>,
    pub error: Option<String>,
    pub checking: bool,
    pub team_focus: FocusHandle,
    pub key_focus: FocusHandle,
}

impl KeySetup {
    pub(crate) fn new(cx: &mut App) -> Self {
        let mut team_id = tahoe::TextField::default();
        if let Some(saved) = crate::api::saved_team_id() {
            team_id.set(saved);
        }
        Self {
            team_id,
            key_id: tahoe::TextField::default(),
            file: None,
            error: None,
            checking: false,
            team_focus: cx.focus_handle(),
            key_focus: cx.focus_handle(),
        }
    }
}

impl MusicApp {
    pub(crate) fn setup_focused(&self, window: &Window) -> bool {
        self.key_setup.team_focus.is_focused(window) || self.key_setup.key_focus.is_focused(window)
    }

    /// Keys for the setup fields. Returns true if used.
    pub(crate) fn setup_key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let team = self.key_setup.team_focus.is_focused(window);
        match k.key.as_str() {
            "enter" => self.connect_key(cx),
            "tab" => {
                let next = if team { &self.key_setup.key_focus } else { &self.key_setup.team_focus };
                window.focus(&next.clone());
            }
            "escape" => window.focus(&self.focus.clone()),
            _ => {
                let field = if team { &mut self.key_setup.team_id } else { &mut self.key_setup.key_id };
                if !field.handle_key(k, window, cx) {
                    return false;
                }
            }
        }
        cx.notify();
        true
    }

    fn choose_key_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Key".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            this.update(cx, |this, cx| {
                if let Some(id) = crate::api::key_id_from_file_name(&path) {
                    this.key_setup.key_id.set(id);
                }
                this.key_setup.file = Some(path);
                this.key_setup.error = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn connect_key(&mut self, cx: &mut Context<Self>) {
        let setup = &mut self.key_setup;
        if setup.checking {
            return;
        }
        let team_id = setup.team_id.text().trim().to_string();
        let key_id = setup.key_id.text().trim().to_string();
        let Some(file) = setup.file.clone() else {
            setup.error = Some("Choose the key's .p8 file first.".into());
            return cx.notify();
        };
        if team_id.is_empty() || key_id.is_empty() {
            setup.error = Some("Fill in your Team ID and the key's Key ID.".into());
            return cx.notify();
        }
        setup.checking = true;
        setup.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::api::connect_musickit(&team_id, &key_id, &file) })
                .await;
            this.update(cx, |this, cx| {
                this.key_setup.checking = false;
                match result {
                    Ok(developer) => this.key_connected(developer, cx),
                    Err(e) => this.key_setup.error = Some(format!("{e:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Switches to the Apple Music API with the new key, without a restart.
    fn key_connected(&mut self, developer: String, cx: &mut Context<Self>) {
        self.catalog = crate::api::Catalog::MusicKit(Auth { developer, user: None });
        self.catalog_note = None;
        self.native_player = None;
        self.native_unavailable = false;
        self.browse = Load::Loading;
        self.load_browse(cx);
        // Picks up a sign-in still in the Keychain from before the key went missing.
        self.restore_sign_in(cx);
    }

    /// The welcome card's body while no key is connected.
    pub(crate) fn render_key_setup(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> Div {
        let setup = &self.key_setup;
        let field =
            |id: &'static str, label: &'static str, field: TextFieldId, focus: &FocusHandle, hint: &'static str| {
                let focused = focus.is_focused(window);
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text_secondary)
                            .child(label),
                    )
                    .child(
                        div()
                            .id(id)
                            .key_context("SearchField")
                            .track_focus(focus)
                            .h(px(32.))
                            .px_2p5()
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .bg(t.field)
                            .border_1()
                            .border_color(if focused { t.accent.opacity(0.6) } else { t.panel_border })
                            .overflow_hidden()
                            .cursor_text()
                            .on_mouse_down(MouseButton::Left, {
                                let focus = focus.clone();
                                move |_, window, _| window.focus(&focus)
                            })
                            .child(
                                tahoe::TextInput::new(cx.entity(), field, focus, t).placeholder(hint).focused(focused),
                            ),
                    )
            };
        let step = |number: &'static str, title: &'static str, body: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap_3()
                .child(
                    div()
                        .size(px(22.))
                        .flex_none()
                        .rounded_full()
                        .bg(t.accent.opacity(0.16))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(t.accent)
                        .child(number),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(body),
                )
        };
        let detail = |text: &'static str| {
            div().text_size(px(12.5)).line_height(px(17.)).text_color(t.text_secondary).child(text)
        };
        let file_name = setup.file.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());

        div()
            .w_full()
            .pt_6()
            .flex()
            .flex_col()
            .gap_4()
            // Why setup is showing when a key was connected before and has gone missing.
            .when_some(self.catalog_note.as_ref().filter(|_| setup.error.is_none()), |d, note| {
                d.child(notice(t, missing_key_message(note)))
            })
            .child(step(
                "1",
                "Create a MusicKit key",
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(detail("In your Apple Developer account, add a key with Media Services enabled."))
                    .child(
                        div()
                            .id("setup-open-keys")
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.accent)
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child("Open Apple Developer")
                            .child(icon("open", 11., t.accent))
                            .on_click(cx.listener(|_, _, _, cx| cx.open_url(KEYS_PAGE))),
                    )
                    .into_any_element(),
            ))
            .child(step(
                "2",
                "Choose the key file",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        pill_button(
                            t,
                            "setup-choose",
                            "plus",
                            if file_name.is_some() { "Change…" } else { "Choose .p8 File…" },
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.choose_key_file(cx))),
                    )
                    .when_some(file_name, |d, name| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .font_family("Menlo")
                                .text_color(t.text_secondary)
                                .child(name),
                        )
                    })
                    .into_any_element(),
            ))
            .child(step(
                "3",
                "Check the IDs",
                div()
                    .flex()
                    .gap_2()
                    .child(field("setup-team", "Team ID", TextFieldId::SetupTeamId, &setup.team_focus, "ABCDE12345"))
                    .child(field(
                        "setup-key",
                        "Key ID",
                        TextFieldId::SetupKeyId,
                        &setup.key_focus,
                        "From the file name",
                    ))
                    .into_any_element(),
            ))
            .when_some(setup.error.clone(), |d, error| d.child(notice(t, error)))
    }

    pub(crate) fn key_setup_actions(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let column = div().w_full().pt_6().flex().flex_col().gap_2();
        let browse = div()
            .id("welcome-browse")
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(t.text_secondary)
            .cursor_pointer()
            .hover(|s| s.text_color(t.text))
            .child("Browse Without Signing In")
            .on_click(cx.listener(|this, _, _, cx| this.finish_welcome(cx)));
        if self.key_setup.checking {
            return column.child(super::welcome::connecting(t, "Checking Your Key…")).child(div().h(px(32.)));
        }
        column
            .child(
                pill_button(t, "setup-connect", "check", "Connect Key", true)
                    .w_full()
                    .h(px(40.))
                    .text_size(px(14.))
                    .on_click(cx.listener(|this, _, _, cx| this.connect_key(cx))),
            )
            .child(browse)
    }
}

/// Says what went wrong with the saved key in words, rather than as an error chain.
fn missing_key_message(note: &str) -> String {
    let detail = note.trim_start_matches("MusicKit disabled: ");
    if detail.contains("No such file")
        && let Some(path) = detail.strip_prefix("reading ").and_then(|rest| rest.split(": ").next())
    {
        let name = std::path::Path::new(path).file_name().map_or(path.into(), |n| n.to_string_lossy());
        return format!(
            "The key file Encore was using, {name}, has been moved or deleted. Choose it again, or a new key."
        );
    }
    format!("Encore couldn't use the saved key: {detail}")
}

fn notice(t: &Theme, text: String) -> impl IntoElement {
    div()
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(10.))
        .bg(t.accent.opacity(0.12))
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(t.text)
        .child(text)
}
