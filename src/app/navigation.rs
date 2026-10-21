//! Moving between pages, with back/forward history and scroll restoration.

use super::*;

impl MusicApp {
    pub(crate) fn go_root(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_direction = NavDirection::Root;
        self.history.clear();
        self.scroll_history.clear();
        if route == Route::Search {
            window.focus(&self.search_focus);
        }
        self.set_route(route, cx);
    }

    pub(crate) fn push(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route == Route::Search {
            self.remember_search();
        }
        if route != self.route {
            self.history.push(self.route.clone());
            self.scroll_history.push(self.scroll.offset());
        }
        self.forward_history.clear();
        self.nav_direction = NavDirection::Forward;
        self.set_route(route, cx);
    }

    /// Goes back and restores where the previous page was scrolled to.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.history.pop() {
            let offset = self.scroll_history.pop();
            self.forward_history.push(self.route.clone());
            self.nav_direction = NavDirection::Back;
            self.set_route(route, cx);
            if let Some(offset) = offset {
                self.scroll.set_offset(offset);
            }
        }
    }

    /// ⌘]: re-open the page Back left.
    pub(crate) fn forward(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.forward_history.pop() {
            self.history.push(self.route.clone());
            self.scroll_history.push(self.scroll.offset());
            self.nav_direction = NavDirection::Forward;
            self.set_route(route, cx);
        }
    }

    pub(crate) fn go_search_page(&mut self, cx: &mut Context<Self>) {
        self.forward_history.clear();
        self.nav_direction = NavDirection::Root;
        self.set_route(Route::Search, cx);
    }

    pub(crate) fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
        self.multi.clear();
        self.route = route;
        self.route_generation += 1;
        self.menu = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.ensure_route_data(cx);
        cx.notify();
    }

    /// Short page name for perf logs.
    pub(crate) fn route_name(&self) -> &'static str {
        match self.route {
            Route::Home => "home",
            Route::New => "new",
            Route::Radio => "radio",
            Route::Search => "search",
            Route::RecentlyAdded => "recently-added",
            Route::Artists => "artists",
            Route::Albums => "albums",
            Route::Songs => "songs",
            Route::AllPlaylists => "all-playlists",
            Route::Favorites => "favorites",
            Route::Album(_) => "album",
            Route::Artist(_) => "artist",
            Route::Playlist(_) => "playlist",
            Route::SeeAll(_) => "see-all",
        }
    }

    /// The name of the page Back returns to, for back buttons.
    pub(crate) fn back_label(&self) -> Option<SharedString> {
        let label: SharedString = match self.history.last()? {
            Route::Home => "Home".into(),
            Route::New => "New".into(),
            Route::Radio => "Radio".into(),
            Route::Search => "Search".into(),
            Route::RecentlyAdded => "Recently Added".into(),
            Route::Artists => "Artists".into(),
            Route::Albums => "Albums".into(),
            Route::Songs => "Songs".into(),
            Route::AllPlaylists => "Playlists".into(),
            Route::Favorites => "Favorite Songs".into(),
            Route::Album(id) => match self.albums.get(id) {
                Some(Load::Ready((album, _))) => album.title.clone().into(),
                _ => "Back".into(),
            },
            Route::Artist(id) => match self.artists.get(id) {
                Some(Load::Ready(detail)) => detail.artist.name.clone().into(),
                _ => "Back".into(),
            },
            Route::Playlist(playlist) => playlist.name.clone().into(),
            Route::SeeAll(page) => page.title.clone(),
        };
        Some(label)
    }

    /// The top-level sidebar entry the current page belongs to.
    pub(crate) fn root_route(&self) -> &Route {
        self.history.first().unwrap_or(&self.route)
    }
}
