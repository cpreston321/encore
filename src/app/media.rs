//! Artwork downloads, artwork colors, and hover/fade animation state.

use super::*;

/// How often to look for artwork to free, and how long unused artwork is kept.
const EVICT_CHECK: Duration = Duration::from_secs(30);
const EVICT_AFTER: Duration = Duration::from_secs(120);
/// Below this many loaded images, nothing is freed.
const KEEP_IMAGES: usize = 100;

impl MusicApp {
    /// Returns the cached file for `url`, starting a download if needed.
    pub(crate) fn image(&self, url: &str, cx: &mut Context<Self>) -> Option<Arc<std::path::Path>> {
        if url.is_empty() {
            return None;
        }
        if let Some(state) = self.images.borrow().get(url) {
            return match state {
                ImageState::Ready(path, _) => Some(self.mark_used(path)),
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

    fn mark_used(&self, path: &Arc<std::path::Path>) -> Arc<std::path::Path> {
        self.image_used.borrow_mut().insert(path.clone(), std::time::Instant::now());
        path.clone()
    }

    /// Frees artwork that hasn't been drawn for a while. gpui keeps every decoded image and its
    /// GPU texture until the app quits, so browsing grows memory without bound. Evicted images
    /// are decoded again from the disk cache if they're needed later.
    ///
    /// Cached views replay their last frame without asking for their images again, so an image
    /// one of them still shows can look unused. Each pass therefore redraws every view first
    /// (notifying the app does that), and only evicts images that redraw didn't use.
    pub(crate) fn start_image_eviction(&mut self, cx: &mut Context<Self>) {
        // ENCORE_TOUR_EVICT_FAST=1 checks every 5 s and frees after 15 s, for testing.
        #[cfg(any(debug_assertions, feature = "tour"))]
        let fast = std::env::var_os("ENCORE_TOUR_EVICT_FAST").is_some();
        #[cfg(not(any(debug_assertions, feature = "tour")))]
        let fast = false;
        let (check, after) =
            if fast { (Duration::from_secs(5), Duration::from_secs(15)) } else { (EVICT_CHECK, EVICT_AFTER) };
        self.evict_after = after;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(check).await;
                let crowded = this.update(cx, |this, cx| {
                    let crowded = this.image_used.borrow().len() > KEEP_IMAGES;
                    if crowded {
                        cx.notify();
                    }
                    crowded
                });
                match crowded {
                    Err(_) => break,
                    Ok(false) => continue,
                    Ok(true) => {}
                }
                // Let the full redraw happen and mark what's on screen.
                cx.background_executor().timer(Duration::from_millis(500)).await;
                if this.update(cx, |this, cx| this.evict_images(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn evict_images(&mut self, cx: &mut Context<Self>) {
        use futures::FutureExt as _;
        let stale: Vec<Arc<std::path::Path>> = {
            let mut used = self.image_used.borrow_mut();
            let stale: Vec<_> =
                used.iter().filter(|(_, at)| at.elapsed() > self.evict_after).map(|(p, _)| p.clone()).collect();
            for path in &stale {
                used.remove(path);
            }
            stale
        };
        for path in &stale {
            let resource = gpui::Resource::Path(path.clone());
            let (task, _) = cx.fetch_asset::<gpui::ImgResourceLoader>(&resource);
            if let Some(Ok(image)) = task.now_or_never() {
                cx.drop_image(image, None);
            }
            cx.remove_asset::<gpui::ImgResourceLoader>(&resource);
        }
        if !stale.is_empty() {
            crate::perf::log(&format!("evicted {} images, {} kept", stale.len(), self.image_used.borrow().len()));
        }
    }

    /// An already-downloaded image (for other windows, which can't start downloads).
    pub(crate) fn cached_image(&self, url: &str) -> Option<Arc<std::path::Path>> {
        match self.images.borrow().get(url) {
            Some(ImageState::Ready(path, _)) => Some(self.mark_used(path)),
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

    /// The pan-and-zoom motion for an artwork image while Animate Artwork is on: a phase that
    /// keeps it out of step with its neighbors, and how far it zooms (`strength`, 0.1 shows
    /// about 90% of the image). The GPU animates it (`StyledImage::motion`), only while the
    /// image is on screen and the window is focused. None when it's off or Reduce Motion is on.
    pub(crate) fn artwork_motion(&self, key: &str, strength: f32) -> Option<(f32, f32)> {
        if !self.animate_artwork || crate::anim::reduce_motion() {
            return None;
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        Some(((hasher.finish() % 10_000) as f32 / 10_000. * std::f32::consts::TAU, strength))
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
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let icc_profile = decoder.icc_profile().ok().flatten();
    let mut pixels = image::DynamicImage::from_decoder(decoder).ok()?.thumbnail(16, 16).to_rgba8();
    // Artwork can be tagged Display P3, and Hsla is sRGB.
    if let Some(icc_profile) = icc_profile {
        gpui::convert_rgba_to_srgb(&mut pixels, &icc_profile);
    }
    let n = (pixels.width() * pixels.height()) as f32;
    let [mut r, mut g, mut b] = [0f32; 3];
    for p in pixels.pixels() {
        r += p[0] as f32;
        g += p[1] as f32;
        b += p[2] as f32;
    }
    Some(gpui::Rgba { r: r / n / 255., g: g / n / 255., b: b / n / 255., a: 1. }.into())
}
