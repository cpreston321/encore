//! Playing songs: the queue, both audio engines, crossfade and gapless handoff, seeking, volume,
//! and the media keys / Control Center integration.

use super::*;
use crate::native::VolumeControl;

impl MusicApp {
    pub(crate) fn play(&mut self, songs: Vec<Song>, start: usize, shuffle: bool, cx: &mut Context<Self>) {
        let songs: Vec<Song> = songs.into_iter().collect();
        if songs.is_empty() {
            return;
        }
        self.resume_at = None;
        self.queue.set(songs, start, shuffle);
        self.start_current(cx);
    }

    pub(crate) fn play_next(&mut self, song: Song, cx: &mut Context<Self>) {
        let was_empty = self.queue.current().is_none();
        self.queue.play_next(song);
        if was_empty {
            self.start_current(cx);
        }
        cx.notify();
    }

    /// Starts the queue's current song, cutting off anything playing (including a fade).
    pub(crate) fn start_current(&mut self, cx: &mut Context<Self>) {
        self.cancel_fade();
        if let Some(audio) = self.audio.borrow_mut().as_mut() {
            audio.stop();
        }
        self.start_track(cx);
    }

    /// Loads the current song into the (empty or reusable) current deck. During a
    /// crossfade it starts silent and the fade brings it up.
    pub(crate) fn start_track(&mut self, cx: &mut Context<Self>) {
        self.load_generation += 1;
        self.playback_error = None;
        self.loading_since = None;
        let Some(song) = self.queue.current().cloned() else {
            self.state = PlayState::Idle;
            return;
        };
        // Prefetch so lyrics are ready the moment Now Playing opens.
        self.ensure_lyrics(&song, cx);
        self.load_spectrum(&song, cx);
        if let Some(player) = self.full_player() {
            *self.audio.borrow_mut() = Some(Audio::Full(player.clone()));
            self.release_web_players();
            player.load(&song.id, self.start_volume());
            self.state = PlayState::Loading;
            self.loading_since = Some(std::time::Instant::now());
            self.ensure_ticker(cx);
            cx.notify();
            return;
        }
        self.start_preview(song, cx);
    }

    /// The signed-in full-song player, created on first use. `None` when signed out or
    /// after full playback proved unavailable this session.
    pub(crate) fn full_player(&mut self) -> Option<FullPlayer> {
        if self.account != Account::SignedIn || self.full_unavailable {
            return None;
        }
        // Any player except the one finishing a crossfade.
        let busy = match self.fade.as_ref().map(|f| &f.outgoing) {
            Some(Audio::Full(p)) => Some(p.clone()),
            _ => None,
        };
        let free = |p: &FullPlayer| busy.as_ref().is_none_or(|b| !b.same(p));
        if let Some(native) = self.native().map(FullPlayer::Native).filter(free) {
            return Some(native);
        }
        if let Some(player) = self.web_players.iter().map(|p| FullPlayer::Web(p.clone())).find(free) {
            return Some(player);
        }
        let (developer, user) = (self.catalog.developer_token()?, self.catalog.user_token()?);
        match WebPlayer::new(developer, user) {
            Ok(player) => {
                let player = Rc::new(player);
                self.web_players.push(player.clone());
                Some(FullPlayer::Web(player))
            }
            Err(e) => {
                eprintln!("full playback unavailable: {e:#}");
                self.full_unavailable = true;
                None
            }
        }
    }

    /// MusicKit's native player, opened on first use. Until the user answers MusicKit's
    /// permission prompt, full songs keep using the web player. `ENCORE_PLAYER=web` forces it.
    fn native(&mut self) -> Option<Rc<crate::native::NativePlayer>> {
        use crate::native::{NativePlayer, Open};
        if let Some(player) = &self.native_player {
            return Some(player.clone());
        }
        if self.native_unavailable {
            return None;
        }
        if std::env::var("ENCORE_PLAYER").as_deref() == Ok("web") {
            self.native_unavailable = true;
            return None;
        }
        match NativePlayer::open(self.catalog.developer_token()?) {
            Open::Ready(player) => {
                if self.native_volume_allowed {
                    player.enable_volume();
                }
                let player = Rc::new(player);
                self.native_player = Some(player.clone());
                Some(player)
            }
            Open::Waiting => None,
            Open::Unavailable => {
                self.native_unavailable = true;
                None
            }
        }
    }

    pub(crate) fn start_preview(&mut self, song: Song, cx: &mut Context<Self>) {
        let generation = self.load_generation;
        let Some(preview) = song.preview_url.clone() else {
            self.state = PlayState::Paused;
            self.playback_error = Some("No preview is available for this song.".into());
            cx.notify();
            return;
        };
        self.state = PlayState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result =
                cx.background_executor().spawn(async move { api::download_cached(&preview, "previews", "m4a") }).await;
            this.update(cx, |this, cx| {
                if this.load_generation != generation {
                    return;
                }
                if let Err(e) = result.and_then(|path| this.load_preview(&path)) {
                    this.state = PlayState::Paused;
                    this.playback_error = Some(format!("Couldn't play preview: {e:#}"));
                } else {
                    this.state = PlayState::Playing;
                    this.apply_resume();
                    this.ensure_ticker(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn load_preview(&mut self, path: &std::path::Path) -> Result<()> {
        let mut slot = self.audio.borrow_mut();
        if !matches!(slot.as_ref(), Some(Audio::Preview(_))) {
            *slot = Some(Audio::Preview(Preview::new()?));
        }
        match slot.as_mut() {
            Some(Audio::Preview(preview)) => preview.load(path, self.start_volume()),
            _ => unreachable!(),
        }
    }

    /// Media keys, headphone buttons and Control Center.
    pub(crate) fn handle_media_command(&mut self, command: crate::media::Command, cx: &mut Context<Self>) {
        use crate::media::Command;
        match command {
            Command::Toggle => self.toggle_play(cx),
            Command::Play if self.state != PlayState::Playing => self.toggle_play(cx),
            Command::Pause if self.state == PlayState::Playing => self.toggle_play(cx),
            Command::Next => self.next(cx),
            Command::Previous => self.previous(cx),
            Command::Seek(t) => {
                if let Some(audio) = self.audio.borrow().as_ref() {
                    audio.seek(t as f32);
                }
            }
            _ => {}
        }
        self.update_now_playing(cx);
    }

    /// Keeps Control Center's Now Playing current.
    pub(crate) fn update_now_playing(&mut self, cx: &mut Context<Self>) {
        let now = self.queue.current().filter(|_| self.state != PlayState::Idle).map(|song| {
            let artwork_path = self.image(&song.artwork.url(600), cx).map(|p| p.to_string_lossy().into_owned());
            crate::media::NowPlaying {
                id: song.id.clone(),
                title: song.title.clone(),
                artist: song.artist.clone(),
                album: song.album.clone(),
                duration: self.duration() as f64,
                position: self.position() as f64,
                playing: matches!(self.state, PlayState::Playing | PlayState::Loading),
                artwork_path,
            }
        });
        self.media.borrow_mut().update(now);
    }

    /// Analyzes the song's preview clip for the player's visualizer.
    pub(crate) fn load_spectrum(&mut self, song: &Song, cx: &mut Context<Self>) {
        let cached = self.spectra.get(&song.id).cloned();
        self.progress.read(cx).spectrum.replace(cached.clone());
        let (Some(url), None) = (song.preview_url.clone(), cached) else { return };
        let id = song.id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    api::download_cached(&url, "previews", "m4a").and_then(|p| crate::spectrum::analyze(&p))
                })
                .await;
            let Ok(spectrum) = result else { return };
            let spectrum = Arc::new(spectrum);
            this.update(cx, |this, cx| {
                this.spectra.insert(id.clone(), spectrum.clone());
                if this.queue.current().is_some_and(|s| s.id == id) {
                    this.progress.read(cx).spectrum.replace(Some(spectrum));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Starts the playback tick if something is playing or loading. It stops by itself once
    /// nothing is, so a paused app has no timers waking it.
    pub(crate) fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !self.needs_tick() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        this.tick(cx);
                        this.update_now_playing(cx);
                        this.progress.update(cx, |p, cx| p.ensure_ticker(cx));
                        if this.state == PlayState::Playing {
                            this.save_session_periodically();
                        }
                        this.ticking = this.needs_tick();
                        this.ticking
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn needs_tick(&self) -> bool {
        matches!(self.state, PlayState::Playing | PlayState::Loading) || self.fade.is_some()
    }

    pub(crate) fn tick(&mut self, cx: &mut Context<Self>) {
        // Now Playing's progress bar and times move with this tick rather than every frame.
        if self.show_now_playing && self.state == PlayState::Playing {
            cx.notify();
        }
        match self.state {
            // A full song has started streaming, or failed to.
            PlayState::Loading if self.loading_since.is_some() => {
                let (playing, error) = match self.audio.borrow().as_ref() {
                    Some(Audio::Full(player)) => (player.is_playing(), player.startup_error()),
                    _ => (false, None),
                };
                let timed_out = self.loading_since.is_some_and(|t| t.elapsed() > Duration::from_secs(20));
                if playing {
                    let native = matches!(self.audio.borrow().as_ref(), Some(Audio::Full(FullPlayer::Native(_))));
                    eprintln!(
                        "full playback started on the {} player ({:.0}s long)",
                        if native { "native" } else { "web" },
                        self.audio.borrow().as_ref().and_then(Audio::duration).unwrap_or(0.)
                    );
                    self.state = PlayState::Playing;
                    self.apply_resume();
                    self.loading_since = None;
                    self.full_failures = 0;
                    cx.notify();
                } else if error.is_some() || timed_out {
                    let reason = error.unwrap_or_else(|| "timed out".into());
                    self.full_failures += 1;
                    // One failure can be a single unavailable song; repeated ones mean full
                    // playback doesn't work for this account, so stop trying.
                    if self.full_failures >= 2 {
                        self.full_unavailable = true;
                    }
                    self.loading_since = None;
                    // Stop the web player too, or a late-starting stream would play over the preview.
                    if let Some(audio) = self.audio.borrow_mut().as_mut() {
                        audio.stop();
                    }
                    if let Some(song) = self.queue.current().cloned() {
                        self.start_preview(song, cx);
                    }
                    self.playback_error =
                        Some(format!("Couldn't stream the full song ({reason}) — playing a preview."));
                    cx.notify();
                }
            }
            PlayState::Playing if self.should_crossfade() => self.begin_crossfade(cx),
            // Only a finished track needs the whole app to redraw; the progress view
            // refreshes itself.
            PlayState::Playing if self.audio.borrow().as_ref().is_some_and(Audio::finished) => {
                match self.queue.track_ended() {
                    Advance::Play => self.start_current(cx),
                    Advance::Stop => {
                        self.state = PlayState::Idle;
                        if let Some(audio) = self.audio.borrow_mut().as_mut() {
                            audio.stop();
                        }
                        self.clear_session();
                    }
                }
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        match self.state {
            PlayState::Playing => {
                self.cancel_fade();
                if let Some(audio) = self.audio.borrow().as_ref() {
                    audio.pause();
                }
                self.state = PlayState::Paused;
                self.save_session();
            }
            PlayState::Paused if self.audio.borrow().as_ref().is_some_and(|a| !a.finished()) => {
                if let Some(audio) = self.audio.borrow().as_ref() {
                    audio.play();
                }
                self.state = PlayState::Playing;
                self.ensure_ticker(cx);
            }
            PlayState::Paused | PlayState::Idle => {
                if self.queue.current().is_some() {
                    self.start_current(cx);
                } else if let Some(browse) = self.browse.ready() {
                    let songs = browse.best_new_songs();
                    self.play(songs, 0, false, cx);
                }
            }
            PlayState::Loading => {}
        }
        cx.notify();
    }

    pub(crate) fn next(&mut self, cx: &mut Context<Self>) {
        if self.queue.current().is_none() {
            return;
        }
        match self.queue.next() {
            Advance::Play => self.start_current(cx),
            Advance::Stop => {
                self.load_generation += 1;
                self.state = PlayState::Idle;
                if let Some(audio) = self.audio.borrow_mut().as_mut() {
                    audio.stop();
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn previous(&mut self, cx: &mut Context<Self>) {
        if self.queue.current().is_none() {
            return;
        }
        if self.queue.previous(self.position()) {
            self.start_current(cx);
        } else {
            self.cancel_fade();
            if let Some(audio) = self.audio.borrow().as_ref() {
                audio.seek(0.);
            }
        }
        cx.notify();
    }

    /// Volume a newly loaded song starts at: silent while crossfading in.
    pub(crate) fn start_volume(&self) -> f32 {
        if self.fade.is_some() { 0. } else { self.volume }
    }

    /// Seconds of overlap with the next song: the crossfade setting, or a near-instant
    /// handoff between consecutive album tracks, which should play gaplessly.
    pub(crate) fn transition_secs(&self) -> Option<f32> {
        let next = self.queue.peek_next()?;
        let current = self.queue.current()?;
        let consecutive = current.album_id.is_some()
            && current.album_id == next.album_id
            && next.track_number == current.track_number + 1;
        if consecutive {
            // Starts the next track just before this one ends; streams need a moment to
            // begin, so this lands close to seamless.
            Some(GAPLESS_LEAD)
        } else if self.crossfade_secs > 0 && self.can_fade_out() {
            Some(self.crossfade_secs as f32)
        } else {
            None
        }
    }

    pub(crate) fn should_crossfade(&self) -> bool {
        if self.fade.is_some() {
            return false;
        }
        let Some(secs) = self.transition_secs() else {
            return false;
        };
        let audio = self.audio.borrow();
        let Some(audio) = audio.as_ref() else {
            return false;
        };
        match audio.duration() {
            // Very short tracks just play out.
            Some(d) if d > secs * 2. => audio.position() >= d - secs,
            _ => false,
        }
    }

    /// Moves the playing song to the fade-out slot and starts the next one silently in a
    /// fresh deck; `step_fade` then trades their volumes.
    pub(crate) fn begin_crossfade(&mut self, cx: &mut Context<Self>) {
        let Some(outgoing) = self.audio.borrow_mut().take() else {
            return;
        };
        let remaining = outgoing.duration().unwrap_or(0.) - outgoing.position();
        #[cfg(any(debug_assertions, feature = "tour"))]
        eprintln!("crossfade begin: pos={:.1} dur={:?}", outgoing.position(), outgoing.duration());
        let secs = self.transition_secs().unwrap_or(self.crossfade_secs as f32);
        // Gapless handoffs are a quick swap, not an audible fade.
        let length = Duration::from_secs_f32(if secs <= GAPLESS_LEAD { 0.12 } else { remaining.clamp(0.5, secs) });
        if self.queue.next() != Advance::Play {
            *self.audio.borrow_mut() = Some(outgoing);
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(30)).await;
                match this.update(cx, |this, cx| this.step_fade(cx)) {
                    Ok(true) => {}
                    _ => break,
                }
            }
        });
        self.fade = Some(Fade { outgoing, length, created: std::time::Instant::now(), started: None, _task: task });
        self.start_track(cx);
        cx.notify();
    }

    /// One frame of an equal-power crossfade. Returns false once it's over.
    pub(crate) fn step_fade(&mut self, _cx: &mut Context<Self>) -> bool {
        let incoming_playing = self.audio.borrow().as_ref().is_some_and(Audio::is_playing);
        let volume = self.volume;
        let Some(fade) = self.fade.as_mut() else {
            return false;
        };
        // Hold the old song at full volume until the new one is actually audible.
        if fade.started.is_none() {
            if incoming_playing || fade.created.elapsed() > Duration::from_secs(4) {
                fade.started = Some(std::time::Instant::now());
            } else {
                return true;
            }
        }
        let t = (fade.started.unwrap().elapsed().as_secs_f32() / fade.length.as_secs_f32()).min(1.);
        #[cfg(any(debug_assertions, feature = "tour"))]
        if (0.44..0.5).contains(&t) || t >= 1. {
            eprintln!(
                "crossfade t={t:.2}: outgoing playing={} incoming playing={incoming_playing}",
                fade.outgoing.is_playing()
            );
        }
        let angle = t * std::f32::consts::FRAC_PI_2;
        fade.outgoing.set_volume(volume * angle.cos());
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.set_volume(volume * angle.sin());
        }
        if t >= 1. {
            if let Some(mut fade) = self.fade.take() {
                fade.outgoing.stop();
            }
            self.release_web_players();
            return false;
        }
        true
    }

    /// With the native player playing, web players are only needed for the next crossfade, and
    /// each keeps a WebKit page with several processes alive, so they're closed until then.
    fn release_web_players(&mut self) {
        let native_playing = matches!(self.audio.borrow().as_ref(), Some(Audio::Full(FullPlayer::Native(_))));
        if native_playing && self.fade.is_none() {
            self.web_players.clear();
        }
    }

    /// Ends any fade at once: the old song stops and the new one jumps to full volume.
    pub(crate) fn cancel_fade(&mut self) {
        if let Some(mut fade) = self.fade.take() {
            fade.outgoing.stop();
            if let Some(audio) = self.audio.borrow().as_ref() {
                audio.set_volume(self.volume);
            }
        }
        self.release_web_players();
    }

    pub(crate) fn jump_ahead(&mut self, n: usize, cx: &mut Context<Self>) {
        if self.queue.jump_ahead(n) {
            self.start_current(cx);
        }
    }

    pub(crate) fn position(&self) -> f32 {
        let resume = self.resume_at.as_ref().map_or(0., |(_, t)| *t);
        match self.state {
            PlayState::Playing | PlayState::Paused => self.audio.borrow().as_ref().map_or(resume, Audio::position),
            _ => 0.,
        }
    }

    pub(crate) fn duration(&self) -> f32 {
        match self.state {
            // Until the stream reports a length, use the catalog's (full songs aren't 30s).
            PlayState::Playing | PlayState::Paused => {
                self.audio.borrow().as_ref().and_then(Audio::duration).unwrap_or_else(|| self.catalog_duration())
            }
            _ => self.catalog_duration(),
        }
    }

    pub(crate) fn is_full_playback(&self) -> bool {
        self.audio.borrow().as_ref().is_some_and(Audio::is_full)
    }

    pub(crate) fn catalog_duration(&self) -> f32 {
        let full = self.audio.borrow().as_ref().is_some_and(Audio::is_full);
        match self.queue.current() {
            Some(s) if full && s.duration_ms > 0 => s.duration_ms as f32 / 1000.,
            _ => 30.,
        }
    }

    /// A volume change from the user. The first one with the native player turns on its
    /// volume control; if that's unavailable, the Mac's output volume changes instead.
    pub(crate) fn change_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.set_volume(volume, cx);
        self.ensure_native_volume(cx);
        let native = match self.audio.borrow().as_ref() {
            Some(Audio::Full(FullPlayer::Native(p))) => Some(p.clone()),
            _ => None,
        };
        if let Some(native) = native
            && native.volume_control() == VolumeControl::Unavailable
        {
            native.set_system_volume(self.volume);
        }
    }

    /// Turns on the native player's volume control the first time it's needed, saying why
    /// macOS is about to ask for System Audio Recording permission.
    pub(crate) fn ensure_native_volume(&mut self, cx: &mut Context<Self>) {
        let Some(native) = self.native_player.clone() else { return };
        if native.volume_control() != VolumeControl::None {
            return;
        }
        if !self.native_volume_allowed {
            self.native_volume_allowed = true;
            self.save_settings();
            self.show_notice(
                "macOS will ask to let Encore record system audio. Encore only uses it to set the volume of the songs it plays."
                    .into(),
                cx,
            );
        }
        native.enable_volume();
    }

    /// The playing song can be faded down: anything but the native player before its volume
    /// control is on.
    fn can_fade_out(&self) -> bool {
        match self.audio.borrow().as_ref() {
            Some(Audio::Full(FullPlayer::Native(p))) => p.volume_control() == VolumeControl::Active,
            _ => true,
        }
    }

    pub(crate) fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.volume = volume.clamp(0., 1.);
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.set_volume(self.volume);
        }
        cx.notify();
    }

    /// Progress scrubs horizontally; volume is a vertical slider (louder toward the top).
    pub(crate) fn drag_to(&mut self, drag: Drag, at: Point<Pixels>, cx: &mut Context<Self>) {
        let fraction = match drag {
            Drag::BigProgress => {
                let b = self.np_progress_bounds.get();
                if b.size.width <= px(0.) {
                    return;
                }
                ((at.x - b.origin.x) / b.size.width).clamp(0., 1.)
            }
            Drag::Progress => {
                let b = self.progress_bounds.get();
                if b.size.width <= px(0.) {
                    return;
                }
                ((at.x - b.origin.x) / b.size.width).clamp(0., 1.)
            }
            Drag::Volume => {
                let b = self.volume_bounds.get();
                if b.size.height <= px(0.) {
                    return;
                }
                (1. - (at.y - b.origin.y) / b.size.height).clamp(0., 1.)
            }
        };
        match drag {
            // Scrubbing only moves the bar; the seek happens once on release (see end_drag),
            // since a stream of seeks makes the full-song player stutter.
            Drag::Progress => self.progress.update(cx, |p, cx| {
                p.set_scrub(Some(fraction));
                cx.notify();
            }),
            Drag::Volume => self.change_volume(fraction, cx),
            Drag::BigProgress => {
                self.np_scrub = Some(fraction);
                cx.notify();
            }
        }
    }

    pub(crate) fn seek_to(&mut self, secs: f32, cx: &mut Context<Self>) {
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.seek(secs);
        }
        // Let the lyrics jump to the new line right away.
        self.lyrics_follow.borrow_mut().user_until = None;
        cx.notify();
    }

    pub(crate) fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag == Some(Drag::BigProgress) {
            self.drag = None;
            if let Some(f) = self.np_scrub.take() {
                let secs = f * self.duration();
                self.seek_to(secs, cx);
            }
            return;
        }
        if self.drag.take() == Some(Drag::Progress) {
            let fraction = self.progress.read(cx).scrub();
            let duration = self.duration();
            if let (Some(f), Some(audio)) = (fraction, self.audio.borrow().as_ref()) {
                audio.seek(f * duration);
            }
            // Keep showing the target until the player reports the new position.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(600)).await;
                this.update(cx, |this, cx| {
                    if this.drag.is_none() {
                        this.progress.update(cx, |p, cx| {
                            p.set_scrub(None);
                            cx.notify();
                        });
                    }
                })
                .ok();
            })
            .detach();
        }
    }
}
