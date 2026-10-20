//! Loading catalog data in the background: charts, albums, artists, playlists, search.

use super::*;

impl MusicApp {
    pub(crate) fn fetch<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        job: impl FnOnce(Catalog) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, Load<T>, &mut Context<Self>) + 'static,
    ) {
        let catalog = self.catalog.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job(catalog) }).await;
            let load = match result {
                Ok(v) => Load::Ready(v),
                Err(e) => {
                    eprintln!("catalog request failed: {e:#}");
                    Load::Failed(format!("{e:#}"))
                }
            };
            this.update(cx, |this, cx| {
                done(this, load, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows the last cached charts immediately, then refreshes them. A failed refresh
    /// keeps the cached copy rather than replacing it with an error.
    pub(crate) fn load_browse(&mut self, cx: &mut Context<Self>) {
        let cache =
            api::cache_dir().join(if self.catalog.is_musickit() { "browse-musickit.json" } else { "browse.json" });
        if self.browse.ready().is_none() {
            self.browse = std::fs::read(&cache)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .map_or(Load::Loading, Load::Ready);
        }
        self.fetch(
            cx,
            |c| c.browse(),
            move |this, load, _| match load {
                Load::Ready(browse) => {
                    if let Ok(json) = serde_json::to_vec(&browse) {
                        let _ = std::fs::create_dir_all(api::cache_dir());
                        let _ = std::fs::write(&cache, json);
                    }
                    crate::perf::milestone("charts ready");
                    this.browse = Load::Ready(browse);
                }
                failed if this.browse.ready().is_none() => this.browse = failed,
                _ => {}
            },
        );
        cx.notify();
    }

    pub(crate) fn ensure_route_data(&mut self, cx: &mut Context<Self>) {
        match self.route.clone() {
            Route::Album(id) if !matches!(self.albums.get(&id), Some(Load::Ready(_) | Load::Loading)) => {
                self.albums.insert(id.clone(), Load::Loading);
                let key = id.clone();
                self.fetch(
                    cx,
                    move |c| c.album(&id),
                    move |this, load, _| {
                        this.albums.insert(key, load);
                    },
                );
            }
            Route::Artist(id) if !matches!(self.artists.get(&id), Some(Load::Ready(_) | Load::Loading)) => {
                self.artists.insert(id.clone(), Load::Loading);
                let key = id.clone();
                self.fetch(
                    cx,
                    move |c| c.artist(&id),
                    move |this, load, _| {
                        this.artists.insert(key, load);
                    },
                );
            }
            Route::Radio if self.catalog.is_musickit() && self.stations.is_none() => {
                self.stations = Some(Load::Loading);
                self.fetch(cx, |c| c.live_stations(), |this, load, _| this.stations = Some(load));
            }
            Route::Playlist(p) if !matches!(self.playlist_tracks.get(&p.id), Some(Load::Ready(_) | Load::Loading)) => {
                self.playlist_tracks.insert(p.id.clone(), Load::Loading);
                let id = p.id.clone();
                self.fetch(
                    cx,
                    move |c| c.playlist_tracks(&p.id),
                    move |this, load, _| {
                        this.playlist_tracks.insert(id, load);
                    },
                );
            }
            _ => {}
        }
    }

    /// Loads a playlist's tracks in the background (e.g. to draw a cover mosaic for
    /// playlists Apple gives no artwork). Safe to call every frame.
    pub(crate) fn ensure_playlist_tracks(&self, playlist: &Playlist, cx: &mut Context<Self>) {
        if self.playlist_tracks.contains_key(&playlist.id)
            || !self.requested_playlists.borrow_mut().insert(playlist.id.clone())
        {
            return;
        }
        let id = playlist.id.clone();
        let key = id.clone();
        self.fetch(
            cx,
            move |c| c.playlist_tracks(&id),
            move |this, load, _| {
                this.playlist_tracks.insert(key, load);
            },
        );
    }

    /// Catalog id for an artist known only by name (looked up once, then cached).
    pub(crate) fn resolve_artist_id(&self, name: &str, cx: &mut Context<Self>) -> Option<String> {
        let key = name.to_lowercase();
        if let Some(found) = self.artist_ids.borrow().get(&key) {
            return found.clone();
        }
        self.artist_ids.borrow_mut().insert(key.clone(), None);
        let name = name.to_string();
        self.fetch(
            cx,
            move |c| c.find_artist(&name),
            move |this, load, _| {
                if let Load::Ready(id) = load {
                    this.artist_ids.borrow_mut().insert(key, id);
                }
            },
        );
        None
    }

    pub(crate) fn ensure_artist(&self, id: &str, cx: &mut Context<Self>) {
        if self.artists.contains_key(id) || !self.requested_artists.borrow_mut().insert(id.to_string()) {
            return;
        }
        let (id, key) = (id.to_string(), id.to_string());
        self.fetch(
            cx,
            move |c| c.artist(&id),
            move |this, load, _| {
                this.artists.insert(key, load);
            },
        );
    }

    /// Opens an artist page, looking the artist up by name when there's no id (chart and
    /// library songs from the Apple Music API don't carry one).
    pub(crate) fn open_artist(&mut self, name: String, id: Option<String>, cx: &mut Context<Self>) {
        if let Some(id) = id {
            return self.push(Route::Artist(id), cx);
        }
        // Collaborations are credited as "A & B" / "A, B"; the first name is the main artist.
        let primary = name.split([',', '&']).next().unwrap_or(&name).trim().to_string();
        let label = primary.clone();
        self.fetch(
            cx,
            move |c| c.find_artist(&primary),
            move |this, load, cx| match load {
                Load::Ready(Some(id)) => this.push(Route::Artist(id), cx),
                Load::Ready(None) => this.show_notice(format!("Couldn't find {label} on Apple Music"), cx),
                Load::Failed(err) => this.show_notice(format!("Couldn't open {label}: {err}"), cx),
                Load::Loading => {}
            },
        );
    }

    pub(crate) fn retry_route(&mut self, cx: &mut Context<Self>) {
        match &self.route {
            Route::Album(id) => {
                self.albums.remove(id);
            }
            Route::Artist(id) => {
                self.artists.remove(id);
            }
            Route::Playlist(p) => {
                self.playlist_tracks.remove(&p.id);
            }
            Route::Radio => {
                self.stations = None;
            }
            Route::Search => {
                self.search_results = None;
                self.schedule_search(cx);
                return;
            }
            Route::RecentlyAdded | Route::Albums | Route::Artists | Route::Songs | Route::AllPlaylists
                if self.account == Account::SignedIn =>
            {
                return self.load_remote_library(cx);
            }
            _ => return self.load_browse(cx),
        }
        self.ensure_route_data(cx);
        cx.notify();
    }

    pub(crate) fn schedule_search(&mut self, cx: &mut Context<Self>) {
        let term = self.search.text().trim().to_string();
        if term.is_empty() {
            self.search_results = None;
            self.search_task = None;
            return;
        }
        if self.search_results.as_ref().is_some_and(|(t, _)| *t == term) {
            return;
        }
        let catalog = self.catalog.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            this.update(cx, |this, cx| {
                this.search_results = Some((term.clone(), Load::Loading));
                cx.notify();
            })
            .ok();
            let query = term.clone();
            let result = cx.background_executor().spawn(async move { catalog.search(&query) }).await;
            this.update(cx, |this, cx| {
                if this.search.text().trim() == term {
                    this.search_results = Some((
                        term,
                        match result {
                            Ok(r) => Load::Ready(r),
                            Err(e) => Load::Failed(format!("{e:#}")),
                        },
                    ));
                    cx.notify();
                }
            })
            .ok();
        }));
    }
}
