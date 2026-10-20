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
