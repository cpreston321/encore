//! Debug-only automated page tour (`ENCORE_TOUR=1`) used to screenshot every page.

use super::*;

impl MusicApp {
    /// Debug-only: steps through every page so screenshots can be taken without driving
    /// the mouse. Each step is announced on stderr as `TOUR <n> <name>`.
    pub(crate) fn start_tour(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let pause = |secs| executor.timer(Duration::from_secs(secs));
            loop {
                pause(1).await;
                let ready = this
                    .update(cx, |this, _| {
                        this.browse.ready().is_some()
                            && (this.account != Account::SignedIn || this.remote.ready().is_some())
                    })
                    .unwrap_or(true);
                if ready {
                    break;
                }
            }
            let Ok(steps) = this.update(cx, |this, _| this.tour_steps()) else {
                return;
            };
            // ENCORE_TOUR_PLAY=1: play silently so the player and progress bar show up.
            if std::env::var_os("ENCORE_TOUR_PLAY").is_some() {
                this.update(cx, |this, cx| {
                    this.show_queue = std::env::var_os("ENCORE_TOUR_QUEUE").is_some();
                    this.show_account = std::env::var_os("ENCORE_TOUR_ACCOUNT").is_some();
                    this.set_volume(0., cx);
                    // ENCORE_TOUR_PLAY=album plays the top new album, for consecutive tracks.
                    let album = std::env::var("ENCORE_TOUR_PLAY").as_deref() == Ok("album");
                    if let Some(browse) = this.browse.ready() {
                        if album && let Some(first) = browse.top_albums.first() {
                            let id = first.id.clone();
                            this.play_album(id, cx);
                        } else {
                            let songs = browse.top_songs.clone();
                            this.play(songs, 1, false, cx);
                        }
                    }
                })
                .ok();
                // ENCORE_TOUR_VOLUME=0.3: once playing, change the volume like the slider does.
                if let Some(volume) = std::env::var("ENCORE_TOUR_VOLUME").ok().and_then(|v| v.parse::<f32>().ok()) {
                    for _ in 0..60 {
                        pause(1).await;
                        if this.update(cx, |this, _| this.state == PlayState::Playing).unwrap_or(true) {
                            break;
                        }
                    }
                    this.update(cx, |this, cx| this.change_volume(volume, cx)).ok();
                }
                // ENCORE_TOUR_SEEK_END=n: jump near the end of n songs in a row, to exercise
                // crossfades (with the native player, both directions of the engine swap).
                let seek_ends = std::env::var("ENCORE_TOUR_SEEK_END").ok().map(|n| n.parse::<usize>().unwrap_or(1));
                for _ in 0..seek_ends.unwrap_or(0) {
                    let mut seeked_song = None;
                    for _ in 0..100 {
                        pause(1).await;
                        seeked_song = this
                            .update(cx, |this, _| {
                                if this.state != PlayState::Playing || this.fade.is_some() {
                                    return None;
                                }
                                let d = this.audio.borrow().as_ref().and_then(Audio::duration)?;
                                let target = d - this.crossfade_secs as f32 - 4.;
                                eprintln!("TOUR seeking to {target:.0}s of {d:.0}s");
                                if let Some(audio) = this.audio.borrow().as_ref() {
                                    audio.seek(target);
                                }
                                this.queue.current().map(|s| s.id.clone())
                            })
                            .unwrap_or(None);
                        if seeked_song.is_some() {
                            break;
                        }
                    }
                    // Wait for the next song to take over before seeking again.
                    for _ in 0..60 {
                        pause(1).await;
                        let moved_on = this
                            .update(cx, |this, _| {
                                this.fade.is_none() && this.queue.current().map(|s| &s.id) != seeked_song.as_ref()
                            })
                            .unwrap_or(true);
                        if moved_on {
                            break;
                        }
                    }
                }
            }
            // ENCORE_TOUR_OVERLAY=palette|shortcuts: show that overlay on top of the tour.
            match std::env::var("ENCORE_TOUR_OVERLAY").as_deref() {
                Ok(overlay @ ("palette" | "palette-selected")) => {
                    let select = overlay == "palette-selected";
                    this.update(cx, |this, cx| {
                        this.search.set(std::env::var("ENCORE_TOUR_QUERY").unwrap_or_else(|_| "zach bryan".into()));
                        if select {
                            this.search.select_all();
                        }
                        this.palette_open = true;
                        this.schedule_search(cx);
                        cx.notify();
                    })
                    .ok();
                }
                Ok(overlay @ ("nowplaying" | "nowplaying-queue")) => {
                    let queue = overlay == "nowplaying-queue";
                    for _ in 0..60 {
                        pause(1).await;
                        let ready = this.update(cx, |this, _| this.state == PlayState::Playing).unwrap_or(true);
                        if ready {
                            break;
                        }
                    }
                    this.update(cx, |this, cx| {
                        use crate::ui::now_playing::NowPlayingTab;
                        this.open_now_playing(
                            Some(if queue { NowPlayingTab::UpNext } else { NowPlayingTab::Lyrics }),
                            cx,
                        );
                        if let Some(audio) = this.audio.borrow().as_ref() {
                            audio.seek(75.);
                        }
                    })
                    .ok();
                }
                Ok("shortcuts") => {
                    this.update(cx, |this, cx| {
                        this.show_shortcuts = true;
                        cx.notify();
                    })
                    .ok();
                }
                _ => {}
            }
            for (n, (name, route, search)) in steps.into_iter().enumerate() {
                // `name@300` means: show the page scrolled down 300px.
                let (label, scroll) = match name.split_once('@') {
                    Some((label, y)) => (label.to_string(), y.parse::<f32>().ok()),
                    None => (name.clone(), None),
                };
                this.update(cx, |this, cx| {
                    eprintln!("TOUR {n} {label}{}", scroll.map(|y| format!("-{y}")).unwrap_or_default());
                    this.history.clear();
                    this.scroll_history.clear();
                    // See All pages are reached from a shelf, so give them a page to go back to.
                    if label.starts_with("see-all") {
                        this.history.push(Route::Home);
                        this.scroll_history.push(point(px(0.), px(0.)));
                    }
                    if let Some(term) = search {
                        this.search.set(term);
                        this.schedule_search(cx);
                    }
                    this.set_route(route, cx);
                })
                .ok();
                // ENCORE_TOUR_SCROLL=1: once the page loads, scroll it smoothly down and back.
                if std::env::var_os("ENCORE_TOUR_SCROLL").is_some() {
                    pause(2).await;
                    this.update(cx, |this, cx| {
                        this.tour_scroll.set(Some(std::time::Instant::now()));
                        cx.notify();
                    })
                    .ok();
                }
                if let Some(y) = scroll {
                    // Let the page load and lay out before scrolling it.
                    pause(3).await;
                    this.update(cx, |this, cx| {
                        this.scroll.set_offset(point(px(0.), px(-y)));
                        cx.notify();
                    })
                    .ok();
                }
                pause(6).await;
            }
            this.update(cx, |this, _| {
                if let Some(native) = &this.native_player {
                    let mut samples = vec![0f32; 2048];
                    let rate = native.samples(&mut samples);
                    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
                    eprintln!(
                        "TOUR native: volume control {:?}, live samples at {rate}Hz, peak {peak:.3}",
                        native.volume_control()
                    );
                }
                let full = this.audio.borrow().as_ref().is_some_and(Audio::is_full);
                eprintln!(
                    "TOUR playback: full={full} state={:?} position={:.1}s duration={:.1}s",
                    this.state,
                    this.position(),
                    this.duration()
                );
            })
            .ok();
            eprintln!("TOUR done");
        })
        .detach();
    }

    /// Advances the scripted scroll by one frame. Called from the page region's render, so
    /// like a real scroll it redraws only the page, at the display's refresh rate.
    pub(crate) fn step_tour_scroll(&self, window: &mut Window) {
        const LENGTH: f32 = 4.;
        let Some(start) = self.tour_scroll.get() else {
            return;
        };
        let t = start.elapsed().as_secs_f32() / LENGTH;
        if t >= 1. {
            self.tour_scroll.set(None);
            return;
        }
        let max = f32::from(self.scroll.max_offset().height).min(2400.);
        let y = max * (1. - (t * std::f32::consts::TAU).cos()) / 2.;
        self.scroll.set_offset(point(px(0.), px(-y)));
        window.request_animation_frame();
    }

    pub(crate) fn tour_steps(&self) -> Vec<(String, Route, Option<String>)> {
        let mut all = self.all_tour_steps();
        // ENCORE_TOUR_ONLY=songs,artists limits the tour to those steps.
        if let Ok(only) = std::env::var("ENCORE_TOUR_ONLY") {
            let names: Vec<&str> = only.split(',').map(str::trim).collect();
            all.retain(|(name, _, _)| names.contains(&name.as_str()));
        }
        if std::env::var("ENCORE_TOUR").as_deref() != Ok("hero") {
            return all;
        }
        // Detail pages only, each at the top, mid-scroll and past the header.
        let mut steps = Vec::new();
        for (name, route, _) in all {
            match name.as_str() {
                "artist" | "catalog-album" | "library-album" | "library-playlist" | "catalog-playlist" => {
                    for y in ["", "@260", "@640"] {
                        steps.push((format!("{name}{y}"), route.clone(), None));
                    }
                }
                "artists" => steps.push((name, route, None)),
                _ => {}
            }
        }
        steps
    }

    pub(crate) fn all_tour_steps(&self) -> Vec<(String, Route, Option<String>)> {
        let mut steps: Vec<(String, Route, Option<String>)> = [
            ("new", Route::New),
            ("home", Route::Home),
            ("radio", Route::Radio),
            ("recently-added", Route::RecentlyAdded),
            ("artists", Route::Artists),
            ("albums", Route::Albums),
            ("songs", Route::Songs),
            ("all-playlists", Route::AllPlaylists),
            ("favorites", Route::Favorites),
        ]
        .into_iter()
        .map(|(n, r)| (n.to_string(), r, None))
        .collect();
        steps.push((
            "search".into(),
            Route::Search,
            Some(std::env::var("ENCORE_TOUR_QUERY").unwrap_or_else(|_| "taylor swift".into())),
        ));
        // ENCORE_TOUR_ARTIST=Name focuses the artist/album steps on one library artist.
        let focus = std::env::var("ENCORE_TOUR_ARTIST").ok().map(|n| n.to_lowercase());
        let focus_album =
            focus.as_ref().and_then(|n| self.lib_albums().iter().find(|a| a.artist.to_lowercase() == *n).cloned());
        // ENCORE_TOUR_ALBUM=<id> adds that album (catalog or library) as an "album" step.
        if let Ok(id) = std::env::var("ENCORE_TOUR_ALBUM") {
            steps.push(("album".into(), Route::Album(id), None));
        }
        if let Some(album) = focus_album.or_else(|| self.lib_albums().first().cloned()) {
            steps.push(("library-album".into(), Route::Album(album.id.clone()), None));
        }
        if let Some(name) = &focus
            && let Some(artist) = self.remote().and_then(|r| r.artists.iter().find(|a| a.name.to_lowercase() == *name))
        {
            steps.push(("artist".into(), Route::Artist(artist.id.clone()), None));
        }
        if let Some(browse) = self.browse.ready() {
            if let Some(album) = browse.top_albums.first() {
                steps.push(("catalog-album".into(), Route::Album(album.id.clone()), None));
            }
            let artist = browse.top_songs.iter().find_map(|s| s.artist_id.clone()).or_else(|| {
                self.remote().and_then(|r| r.artists.iter().find(|a| !a.id.starts_with("r.")).map(|a| a.id.clone()))
            });
            if let Some(id) = artist.filter(|_| focus.is_none()) {
                steps.push(("artist".into(), Route::Artist(id), None));
            }
            if let Some(p) = browse.top_playlists.first() {
                steps.push(("catalog-playlist".into(), Route::Playlist(p.clone()), None));
            }
        }
        if let Some(p) = self.lib_playlists().first() {
            steps.push(("library-playlist".into(), Route::Playlist(p.clone()), None));
        }
        if let Some(browse) = self.browse.ready() {
            use crate::app::{SeeAll, SeeAllItems};
            let page = |title: &str, items| Route::SeeAll(Arc::new(SeeAll { title: title.to_string().into(), items }));
            steps.push((
                "see-all-songs".into(),
                page("Top Songs", SeeAllItems::Songs(Arc::new(browse.top_songs.clone()))),
                None,
            ));
            steps.push((
                "see-all-albums".into(),
                page("Top Albums", SeeAllItems::Albums(Arc::new(browse.top_albums.clone()))),
                None,
            ));
        }
        steps
    }
}

/// Stand-in artwork (`ENCORE_TOUR_PLACEHOLDER_ART=1`) so published screenshots don't carry
/// album covers or artist photos. Seeded by the URL without its size segment, so every size
/// of one cover gets the same picture.
pub(crate) fn placeholder_art(url: &str) -> Result<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.rsplit_once('/').map_or(url, |(base, _)| base).hash(&mut hasher);
    let seed = hasher.finish();
    let dir = api::cache_dir().join("art-placeholder");
    let path = dir.join(format!("{seed:016x}.png"));
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)?;
    let hue = (seed % 360) as f32;
    let from = hsl(hue, 0.55, 0.52);
    let to = hsl((hue + 40. + (seed >> 16) as f32 % 80.) % 360., 0.6, 0.32);
    let (gx, gy) = (0.25 + ((seed >> 24) % 50) as f32 / 100., 0.2 + ((seed >> 32) % 40) as f32 / 100.);
    const SIZE: u32 = 600;
    let img = image::RgbImage::from_fn(SIZE, SIZE, |x, y| {
        let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
        let t = (u + v) / 2.;
        let glow = (1. - ((u - gx).powi(2) + (v - gy).powi(2)).sqrt() / 0.55).max(0.) * 0.35;
        image::Rgb(std::array::from_fn(|i| {
            let c = from[i] + (to[i] - from[i]) * t;
            ((c + (1. - c) * glow) * 255.) as u8
        }))
    });
    let tmp = path.with_extension(format!("png.part{}", std::process::id()));
    img.save_with_format(&tmp, image::ImageFormat::Png)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

fn hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.) % 2. - 1.).abs());
    let m = l - c / 2.;
    let (r, g, b) = match (h / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    [r + m, g + m, b + m]
}
