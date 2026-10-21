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
        // Whatever the pointer was over goes with the page and never sees it leave, so
        // nothing would fade its hover back out. The sidebar's keys stay.
        self.hovered_link.replace(None);
        let mut fx = self.fx.borrow_mut();
        fx.hovers.retain(|key, _| key.starts_with("nav-"));
        fx.page_entered = std::time::Instant::now();
        drop(fx);
        self.route = route;
        self.route_generation += 1;
        // Shelves of the page being left would otherwise still claim the swipes over where
        // they were.
        self.shelves.borrow_mut().clear();
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

    /// The top-level sidebar entry the current page belongs to.
    pub(crate) fn root_route(&self) -> &Route {
        self.history.first().unwrap_or(&self.route)
    }
}
