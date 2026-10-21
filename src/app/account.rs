//! Apple Music sign-in and the user's library (Apple's when signed in, local otherwise).

use super::*;

/// How long requests wait for the saved user token before going out without it.
const TOKEN_WAIT: Duration = Duration::from_secs(2);

impl MusicApp {
    /// Reads the saved Music User Token off the main thread: macOS can show a Keychain
    /// prompt (e.g. after an app update), and the window shouldn't wait on it. If a cached
    /// library exists we show it right away as signed in; the token follows.
    pub(crate) fn restore_sign_in(&mut self, cx: &mut Context<Self>) {
        if remote_cache_path().exists() {
            self.account = Account::SignedIn;
            self.load_remote_library_cache();
            if let Some(home) = std::fs::read(api::cache_dir().join("personal-home.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<crate::model::PersonalHome>(&b).ok())
            {
                self.personal = Some(Load::Ready(Some(home)));
            }
        }
        if self.catalog.is_musickit() {
            *self.awaiting_token.borrow_mut() = Some(Vec::new());
            // A Keychain prompt can wait on the user indefinitely; past this, requests go out
            // without the token, and what fails for want of it is asked for again once it's in.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TOKEN_WAIT).await;
                this.update(cx, |this, cx| this.send_waiting_requests(cx)).ok();
            })
            .detach();
        }
        cx.spawn(async move |this, cx| {
            let token = cx.background_executor().spawn(async { auth::stored_user_token() }).await;
            crate::perf::milestone("keychain read");
            this.update(cx, |this, cx| {
                let waited_too_long = this.awaiting_token.borrow().is_none();
                match token {
                    Some(token) => {
                        this.catalog.set_user_token(Some(token));
                        this.account = Account::SignedIn;
                        if waited_too_long {
                            this.retry_failed_loads();
                        }
                        this.load_remote_library(cx);
                        this.load_personal_home(cx);
                        this.ensure_route_data(cx);
                    }
                    None => {
                        this.account = Account::SignedOut;
                        this.remote = Load::Loading;
                    }
                }
                this.send_waiting_requests(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn send_waiting_requests(&mut self, cx: &mut Context<Self>) {
        let waiting = self.awaiting_token.borrow_mut().take().unwrap_or_default();
        for fetch in waiting {
            fetch(self, cx);
        }
    }

    /// Lets loads that failed while signed out, or before the token arrived, run again.
    fn retry_failed_loads(&mut self) {
        self.playlist_tracks.retain(|_, load| !matches!(load, Load::Failed(_)));
        self.requested_playlists.borrow_mut().clear();
        self.albums.retain(|_, load| !matches!(load, Load::Failed(_)));
    }

    /// Apple doesn't expose Apple Music profile photos to third-party apps, so the user
    /// picks one; a copy lives in the app's data folder.
    pub(crate) fn choose_photo(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Photo".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            let copied = cx
                .background_executor()
                .spawn(async move {
                    use image::{ImageDecoder, ImageEncoder};
                    // Re-encode so any image format works and the copy stays small, keeping the
                    // color profile so photos in Display P3 aren't drawn as sRGB.
                    let mut decoder = image::ImageReader::open(&source)?.with_guessed_format()?.into_decoder()?;
                    let icc_profile = decoder.icc_profile().ok().flatten();
                    let image = image::DynamicImage::from_decoder(decoder)?.thumbnail(256, 256).into_rgba8();
                    std::fs::create_dir_all(api::data_dir())?;
                    let path = api::data_dir().join("profile-photo.png");
                    let mut encoder =
                        image::codecs::png::PngEncoder::new(std::io::BufWriter::new(std::fs::File::create(&path)?));
                    if let Some(icc_profile) = icc_profile {
                        encoder.set_icc_profile(icc_profile).ok();
                    }
                    encoder.write_image(
                        image.as_raw(),
                        image.width(),
                        image.height(),
                        image::ExtendedColorType::Rgba8,
                    )?;
                    anyhow::Ok(path)
                })
                .await;
            this.update(cx, |this, cx| match copied {
                Ok(path) => {
                    // A new path each time so gpui's image cache doesn't show the old one.
                    let stamped = path.with_file_name(format!(
                        "profile-photo-{}.png",
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
                    ));
                    let _ = std::fs::rename(&path, &stamped);
                    remove_old_photos(&stamped);
                    this.avatar = Some(Arc::from(stamped.as_path()));
                    cx.notify();
                }
                Err(e) => this.show_notice(format!("Couldn't use that image: {e:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn remove_photo(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.avatar.take() {
            let _ = std::fs::remove_file(&*path);
        }
        cx.notify();
    }

    /// Home's personal shelves: the cached copy shows at once (no layout shift on launch),
    /// then a fresh copy replaces it when Apple answers.
    pub(crate) fn load_personal_home(&mut self, cx: &mut Context<Self>) {
        let path = api::cache_dir().join("personal-home.json");
        if self.personal.is_none() {
            self.personal = Some(
                std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<crate::model::PersonalHome>(&b).ok())
                    .map_or(Load::Loading, |h| Load::Ready(Some(h))),
            );
        }
        self.fetch(
            cx,
            |c| c.personal_home(),
            move |this, load, _| match load {
                Load::Ready(Some(home)) => {
                    if let Ok(json) = serde_json::to_vec(&home) {
                        let _ = std::fs::write(&path, json);
                    }
                    this.personal = Some(Load::Ready(Some(home)));
                }
                // Keep showing the cached shelves if a refresh fails.
                other if !matches!(this.personal, Some(Load::Ready(Some(_)))) => this.personal = Some(other),
                _ => {}
            },
        );
    }

    pub(crate) fn load_remote_library_cache(&mut self) {
        if self.remote.ready().is_none()
            && let Some(library) = std::fs::read(remote_cache_path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<RemoteLibrary>(&bytes).ok())
        {
            self.remote = Load::Ready(library);
        }
    }

    pub(crate) fn sign_in(&mut self, cx: &mut Context<Self>) {
        let Some(developer_token) = self.catalog.developer_token().map(str::to_string) else {
            self.show_key_help = true;
            cx.notify();
            return;
        };
        self.sign_in_error = None;
        let server = match auth::SignInServer::start(&developer_token) {
            Ok(server) => server,
            Err(e) if self.welcome_signing => {
                self.welcome_signing = false;
                self.sign_in_error = Some(format!("{e:#}"));
                return cx.notify();
            }
            Err(e) => return self.show_notice(format!("Couldn't start sign-in: {e:#}"), cx),
        };
        cx.open_url(&server.url);
        // A new attempt replaces any earlier one still waiting.
        if let Some(stop) = self.sign_in_stop.replace(server.stop_flag()) {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.account = Account::SigningIn;
        self.sign_in_task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { server.wait_for_token() }).await;
            this.update(cx, |this, cx| match result.and_then(|token| auth::store_user_token(&token).map(|_| token)) {
                Ok(token) => {
                    this.catalog.set_user_token(Some(token));
                    this.account = Account::SignedIn;
                    this.retry_failed_loads();
                    this.load_remote_library(cx);
                    this.load_personal_home(cx);
                    if !this.welcome_signing {
                        this.show_notice("Signed in to Apple Music".into(), cx);
                    }
                }
                Err(e) => {
                    this.account = Account::SignedOut;
                    if this.welcome_signing {
                        this.sign_in_error = Some(format!("{e:#}"));
                        this.welcome_signing = false;
                    } else {
                        this.show_notice(format!("Sign-in didn't complete: {e:#}"), cx);
                    }
                }
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        // Stops the local sign-in server too, not just our wait on it.
        if let Some(stop) = self.sign_in_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.sign_in_task = None;
        if self.account == Account::SigningIn {
            self.account = Account::SignedOut;
        }
        self.welcome_signing = false;
        cx.notify();
    }

    pub(crate) fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.cancel_fade();
        self.load_generation += 1;
        if let Some(mut audio) = self.audio.borrow_mut().take() {
            audio.stop();
        }
        self.state = PlayState::Idle;
        self.queue.clear();
        self.clear_session();
        self.progress.read(cx).set_resume(None);
        self.show_now_playing = false;
        self.menu = None;
        self.show_welcome = true;
        self.update_now_playing(cx);
        self.web_players.clear();
        self.native_player = None;
        self.native_unavailable = false;
        self.personal = None;
        let _ = std::fs::remove_file(api::cache_dir().join("personal-home.json"));
        self.full_unavailable = false;
        let _ = std::fs::remove_file(remote_cache_path());
        auth::clear_user_token();
        self.catalog.set_user_token(None);
        self.account = Account::SignedOut;
        self.remote = Load::Loading;
        self.playlist_tracks.retain(|id, _| !id.starts_with("p."));
        self.albums.retain(|id, _| !id.starts_with("l."));
        cx.notify();
    }

    /// Shows the cached copy of the library immediately, then refreshes it. Favorites come
    /// in a second pass since they need a ratings lookup per 100 songs.
    pub(crate) fn load_remote_library(&mut self, cx: &mut Context<Self>) {
        self.load_remote_library_cache();
        let started = std::time::Instant::now();
        self.fetch(
            cx,
            |c| c.library(),
            move |this, load, cx| {
                match load {
                    // A rejected token could also be one rejected endpoint; confirm with a
                    // second request before throwing the sign-in away.
                    Load::Failed(err) if err.contains("http status: 401") || err.contains("http status: 403") => {
                        eprintln!("library request rejected: {err}");
                        this.fetch(
                            cx,
                            |c| c.user_token_valid(),
                            |this, load, cx| match load {
                                Load::Ready(false) => {
                                    if this.welcome_signing {
                                        this.welcome_signing = false;
                                        this.sign_in_error = Some("Apple Music didn't accept the sign-in.".into());
                                    }
                                    this.sign_out_expired();
                                }
                                _ => {
                                    if this.remote.ready().is_none() {
                                        this.remote = Load::Failed(
                                            "Apple Music refused the library request. Try again later.".into(),
                                        );
                                    }
                                    this.welcome_library_settled(cx);
                                    cx.notify();
                                }
                            },
                        );
                    }
                    Load::Failed(err) => {
                        eprintln!("library failed after {:?}: {err}", started.elapsed());
                        // Keep showing the last good copy if a refresh fails.
                        if this.remote.ready().is_none() {
                            this.remote = Load::Failed(err);
                        }
                        this.welcome_library_settled(cx);
                    }
                    Load::Ready(mut library) => {
                        eprintln!(
                            "library: {} songs, {} albums, {} playlists in {:?}",
                            library.songs.len(),
                            library.albums.len(),
                            library.playlists.len(),
                            started.elapsed()
                        );
                        if let Some(old) = this.remote.ready() {
                            library.favorites = old.favorites.clone();
                        }
                        crate::perf::milestone("library ready");
                        let songs = library.songs.clone();
                        this.remote = Load::Ready(library);
                        this.library_updated = Some(std::time::Instant::now());
                        this.save_remote_cache();
                        this.load_favorites(songs, cx);
                        this.welcome_library_settled(cx);
                    }
                    Load::Loading => {}
                }
            },
        );
        cx.notify();
    }

    pub(crate) fn load_favorites(&mut self, songs: Arc<Vec<Song>>, cx: &mut Context<Self>) {
        self.fetch(
            cx,
            move |c| c.library_favorites(&songs),
            |this, load, _| match load {
                Load::Ready(favorites) => {
                    if let Load::Ready(library) = &mut this.remote {
                        library.favorites = Arc::new(favorites);
                    }
                    this.save_remote_cache();
                }
                Load::Failed(err) => eprintln!("favorites failed: {err}"),
                Load::Loading => {}
            },
        );
    }

    pub(crate) fn save_remote_cache(&self) {
        if let Some(library) = self.remote.ready()
            && let Ok(json) = serde_json::to_vec(library)
        {
            let _ = std::fs::create_dir_all(api::cache_dir());
            let _ = std::fs::write(remote_cache_path(), json);
        }
    }

    pub(crate) fn sign_out_expired(&mut self) {
        let _ = std::fs::remove_file(remote_cache_path());
        auth::clear_user_token();
        self.catalog.set_user_token(None);
        self.account = Account::SignedOut;
        self.remote = Load::Loading;
        self.notice = Some("Your Apple Music session expired. Sign in again to see your library.".into());
    }

    pub(crate) fn remote(&self) -> Option<&RemoteLibrary> {
        if self.account == Account::SignedIn { self.remote.ready() } else { None }
    }

    pub(crate) fn lib_songs(&self) -> Arc<Vec<Song>> {
        self.remote().map_or_else(|| Arc::new(self.library.songs.clone()), |r| r.songs.clone())
    }

    pub(crate) fn lib_albums(&self) -> &[Album] {
        self.remote().map_or(&self.library.albums, |r| &r.albums)
    }

    /// Changes whenever the library contents might have: cheap key for derived caches.
    pub(crate) fn library_key(&self) -> (usize, usize, usize, usize, usize) {
        match self.remote() {
            Some(r) => (1, Arc::as_ptr(&r.songs) as usize, r.albums.len(), r.artists.len(), r.albums.as_ptr() as usize),
            None => (0, self.library.songs.len(), self.library.albums.len(), 0, self.library.albums.as_ptr() as usize),
        }
    }

    /// Library albums sorted by title, computed once per library change.
    pub(crate) fn sorted_albums(&self) -> Arc<Vec<Album>> {
        let key = self.library_key();
        if let Some((k, v)) = &*self.sorted_albums_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let mut albums = self.lib_albums().to_vec();
        albums.sort_by_cached_key(|a| a.title.to_lowercase());
        let albums = Arc::new(albums);
        *self.sorted_albums_cache.borrow_mut() = Some((key, albums.clone()));
        albums
    }

    /// Cached rows for the Artists page (see `ui::library`).
    pub(crate) fn library_artist_rows(&self) -> Arc<Vec<(String, Option<String>, crate::model::Artwork)>> {
        let key = self.library_key();
        if let Some((k, v)) = &*self.artist_rows_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let rows = Arc::new(self.library_artist_rows_uncached());
        *self.artist_rows_cache.borrow_mut() = Some((key, rows.clone()));
        rows
    }

    pub(crate) fn lib_recently_added(&self) -> Vec<Album> {
        match self.remote() {
            Some(r) if !r.recently_added.is_empty() => r.recently_added.clone(),
            Some(r) => r.albums.clone(),
            None => self.library.albums.clone(),
        }
    }

    pub(crate) fn lib_playlists(&self) -> Vec<Playlist> {
        self.remote().map_or_else(|| self.library.playlists.clone(), |r| r.playlists.clone())
    }

    /// Apple's own auto-updating "Favorite Songs" playlist, when the library has one.
    pub(crate) fn is_apple_favorites(&self, playlist: &Playlist) -> bool {
        self.remote().is_some() && playlist.id.starts_with("p.") && playlist.name == "Favorite Songs"
    }

    /// Where the sidebar's Favorite Songs goes: Apple's playlist (its real order) if present.
    pub(crate) fn favorites_route(&self) -> Route {
        self.lib_playlists().into_iter().find(|p| self.is_apple_favorites(p)).map_or(Route::Favorites, Route::Playlist)
    }

    pub(crate) fn lib_favorites(&self) -> Arc<Vec<Song>> {
        self.remote().map_or_else(|| Arc::new(self.library.favorites.clone()), |r| r.favorites.clone())
    }

    pub(crate) fn is_favorite(&self, song: &Song) -> bool {
        match self.remote() {
            Some(r) => r.favorites.iter().any(|s| s.id == song.id),
            None => self.library.is_favorite(&song.id),
        }
    }

    /// Signed in, this sets the song's Apple Music rating and updates the list optimistically,
    /// rolling back if Apple rejects it.
    pub(crate) fn toggle_favorite(&mut self, song: &Song, cx: &mut Context<Self>) {
        let Load::Ready(remote) = &mut self.remote else {
            self.library.toggle_favorite(song);
            return cx.notify();
        };
        if self.account != Account::SignedIn {
            self.library.toggle_favorite(song);
            return cx.notify();
        }
        let favorite = !remote.favorites.iter().any(|s| s.id == song.id);
        if favorite {
            Arc::make_mut(&mut remote.favorites).insert(0, song.clone());
        } else {
            Arc::make_mut(&mut remote.favorites).retain(|s| s.id != song.id);
        }
        let id = song.id.clone();
        let rollback = song.clone();
        self.fetch(
            cx,
            move |c| c.set_favorite(&id, favorite),
            move |this, load, _| {
                if let Load::Failed(err) = load {
                    if let Load::Ready(remote) = &mut this.remote {
                        if favorite {
                            Arc::make_mut(&mut remote.favorites).retain(|s| s.id != rollback.id);
                        } else {
                            Arc::make_mut(&mut remote.favorites).insert(0, rollback);
                        }
                    }
                    this.notice = Some(format!("Couldn't update favorite: {err}"));
                }
            },
        );
        cx.notify();
    }

    pub(crate) fn song_in_library(&self, song: &Song) -> bool {
        match self.remote() {
            Some(r) => r.songs.iter().any(|s| s.id == song.id),
            None => self.library.has_song(&song.id),
        }
    }

    pub(crate) fn album_in_library(&self, album: &Album) -> bool {
        match self.remote() {
            Some(r) => r.albums.iter().any(|a| a.id == album.id || (!album.url.is_empty() && a.url == album.url)),
            None => self.library.has_album(&album.id),
        }
    }

    pub(crate) fn playlist_in_library(&self, playlist: &Playlist) -> bool {
        match self.remote() {
            Some(r) => playlist.id.starts_with("p.") || r.playlists.iter().any(|p| p.id == playlist.id),
            None => self.library.has_playlist(&playlist.id),
        }
    }

    /// Adds to the Apple Music library when signed in (Apple's API can add but not remove),
    /// otherwise toggles the item in the local library.
    pub(crate) fn add_to_library(&mut self, kind: &'static str, id: String, label: String, cx: &mut Context<Self>) {
        let request_id = id.clone();
        self.fetch(
            cx,
            move |c| c.add_to_library(kind, &request_id),
            move |this, load, _| match load {
                Load::Failed(err) => this.notice = Some(format!("Couldn't add {label}: {err}")),
                _ => {
                    this.notice = Some(format!("Added {label} to your library"));
                    this.notice_task = None;
                }
            },
        );
        // Apple applies library additions asynchronously; refresh after a short delay.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            this.update(cx, |this, cx| {
                this.notice = None;
                this.load_remote_library(cx);
            })
            .ok();
        })
        .detach();
    }
}
