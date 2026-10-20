//! Persisted preferences (crossfade, folded sidebar sections, recent searches).

use super::*;

impl MusicApp {
    pub(crate) fn save_settings(&self) {
        Settings {
            crossfade_secs: self.crossfade_secs,
            collapsed: self.collapsed_sections.iter().cloned().collect(),
            recent_searches: self.recent_searches.clone(),
        }
        .save();
    }

    pub(crate) fn toggle_section(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.collapsed_sections.remove(name) {
            self.collapsed_sections.insert(name.to_string());
        }
        self.save_settings();
        cx.notify();
    }

    /// Eased 0 (folded) – 1 (open) for a sidebar section.
    pub(crate) fn section_open(&self, name: &'static str) -> f32 {
        let open = !self.collapsed_sections.contains(name);
        let mut fx = self.fx.borrow_mut();
        let p = fx.sections.entry(name).or_insert_with(|| {
            let d = Duration::from_millis(220);
            if open { crate::anim::Presence::new_open(d) } else { crate::anim::Presence::new(d) }
        });
        p.set(open);
        p.value()
    }

    pub(crate) fn set_crossfade(&mut self, secs: u32, cx: &mut Context<Self>) {
        self.crossfade_secs = secs;
        self.save_settings();
        cx.notify();
    }
}
