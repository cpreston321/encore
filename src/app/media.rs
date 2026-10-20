//! Artwork downloads, artwork colors, and hover/fade animation state.

use super::*;

impl MusicApp {
    /// Returns the cached file for `url`, starting a download if needed.
    pub(crate) fn image(&self, url: &str, cx: &mut Context<Self>) -> Option<Arc<std::path::Path>> {
        if url.is_empty() {
            return None;
        }
        if let Some(state) = self.images.borrow().get(url) {
            return match state {
                ImageState::Ready(path, _) => Some(path.clone()),
                _ => None,
            };
        }
        self.images.borrow_mut().insert(url.to_string(), ImageState::Loading);
        let url = url.to_string();
        cx.spawn(async move |this, cx| {
            let fetch_url = url.clone();
            let result = cx.background_executor().spawn(async move { fetch_art(&fetch_url) }).await;
            this.update(cx, |this, cx| {
                let state = match result {
                    Ok(path) => ImageState::Ready(Arc::from(path.as_path()), std::time::Instant::now()),
                    Err(_) => ImageState::Failed,
                };
                this.images.borrow_mut().insert(url, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// The artwork's average color (computed once, in the background, from a small copy).
    pub(crate) fn art_color(&self, artwork: &crate::model::Artwork, cx: &mut Context<Self>) -> Option<gpui::Hsla> {
        if artwork.is_empty() {
            return None;
        }
        let url = artwork.url(60);
        if let Some(color) = self.art_colors.borrow().get(&url) {
            return *color;
        }
        self.art_colors.borrow_mut().insert(url.clone(), None);
        cx.spawn(async move |this, cx| {
            let fetch_url = url.clone();
            let color = cx
                .background_executor()
                .spawn(async move { fetch_art(&fetch_url).ok().and_then(|path| average_color(&path)) })
                .await;
            this.update(cx, |this, cx| {
                this.art_colors.borrow_mut().insert(url, color);
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// An already-downloaded image (for other windows, which can't start downloads).
    pub(crate) fn cached_image(&self, url: &str) -> Option<Arc<std::path::Path>> {
        match self.images.borrow().get(url) {
            Some(ImageState::Ready(path, _)) => Some(path.clone()),
            _ => None,
        }
    }

    /// Eased 0–1 hover amount for an element registered with `set_hover`.
    pub(crate) fn hover_value(&self, key: &str) -> f32 {
        self.fx.borrow().hovers.get(key).map_or(0., |p| p.value())
    }

    /// Records a hover change; returns true if a redraw is needed.
    pub(crate) fn set_hover(&self, key: SharedString, hovered: bool) -> bool {
        let mut fx = self.fx.borrow_mut();
        let entry = fx.hovers.entry(key).or_insert_with(|| crate::anim::Presence::new(Duration::from_millis(160)));
        let before = entry.value();
        entry.set(hovered);
        hovered || before > 0.
    }

    /// The part of an artwork image to show while Animate Artwork is on: a slow zoom that
    /// breathes and a pan that wanders, out of phase for each image so neighbors don't move
    /// together. `strength` is the zoom (0.1 shows ~90% of the image). None when it's off,
    /// Reduce Motion is on, or the window isn't focused. `root` says which view is drawing,
    /// so it's the one that asks for the next frame.
    pub(crate) fn artwork_motion(&self, key: &str, strength: f32, root: bool) -> Option<gpui::Bounds<f32>> {
        if !self.animate_artwork || !self.window_active.get() || crate::anim::reduce_motion() {
            return None;
        }
        {
            let mut fx = self.fx.borrow_mut();
            if root { fx.motion_root = true } else { fx.motion_page = true }
        }
        use std::{
            f32::consts::TAU,
            hash::{Hash, Hasher},
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        let phase = (hasher.finish() % 10_000) as f32 / 10_000. * TAU;
        let t = self.motion_epoch.elapsed().as_secs_f32();
        let zoom = 1. + strength * (0.75 + 0.25 * (t * TAU / 23. + phase).sin());
        let side = 1. / zoom;
        let x = 0.5 + 0.5 * (t * TAU / 31. + phase * 1.7).sin();
        let y = 0.5 + 0.5 * (t * TAU / 41. + phase * 2.3).cos();
        Some(gpui::Bounds { origin: gpui::point((1. - side) * x, (1. - side) * y), size: gpui::size(side, side) })
    }

    /// True for images that arrived in the last moment, so they fade in rather than pop;
    /// images already on disk when a page opens appear immediately.
    pub(crate) fn image_is_fresh(&self, url: &str) -> bool {
        matches!(self.images.borrow().get(url), Some(ImageState::Ready(_, at)) if at.elapsed() < Duration::from_millis(400))
    }
}

fn fetch_art(url: &str) -> Result<std::path::PathBuf> {
    #[cfg(any(debug_assertions, feature = "tour"))]
    if std::env::var_os("ENCORE_TOUR_PLACEHOLDER_ART").is_some() {
        return super::tour::placeholder_art(url);
    }
    api::download_cached(url, "art", "jpg")
}

fn average_color(path: &std::path::Path) -> Option<gpui::Hsla> {
    let pixels = image::open(path).ok()?.thumbnail(16, 16).to_rgb8();
    let n = (pixels.width() * pixels.height()) as f32;
    let [mut r, mut g, mut b] = [0f32; 3];
    for p in pixels.pixels() {
        r += p[0] as f32;
        g += p[1] as f32;
        b += p[2] as f32;
    }
    Some(gpui::Rgba { r: r / n / 255., g: g / n / 255., b: b / n / 255., a: 1. }.into())
}
