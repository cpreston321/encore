//! See All pages: everything in a shelf whose header was clicked.

use super::*;
use crate::app::{SeeAll, SeeAllItems};

impl MusicApp {
    /// A shelf title with a chevron that opens the shelf's See All page. `items` builds the full
    /// list from the app's state when clicked, so rendering doesn't copy it every frame.
    pub(crate) fn section_link(
        &self,
        t: &Theme,
        title: impl Into<SharedString>,
        items: impl Fn(&MusicApp) -> Option<SeeAllItems> + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = title.into();
        let page_title = title.clone();
        self.section_header_clickable(t, title, cx, move |this| {
            items(this).map(|items| Route::SeeAll(Arc::new(SeeAll { title: page_title.clone(), items })))
        })
    }

    /// A shelf title with a chevron that opens `route`.
    pub(crate) fn section_link_to(
        &self,
        t: &Theme,
        title: impl Into<SharedString>,
        route: Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.section_header_clickable(t, title.into(), cx, move |_| Some(route.clone()))
    }

    fn section_header_clickable(
        &self,
        t: &Theme,
        title: SharedString,
        cx: &mut Context<Self>,
        target: impl Fn(&MusicApp) -> Option<Route> + 'static,
    ) -> impl IntoElement {
        tahoe::SectionHeader::new(ElementId::Name(format!("see-all-{title}").into()), title)
            .inset(PAD)
            .theme(*t)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(route) = target(this) {
                    this.push(route, cx);
                }
            }))
    }

    pub(crate) fn render_see_all(
        &self,
        t: &Theme,
        page: &SeeAll,
        content_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (_, tile) = grid_layout(content_w, 170., usize::MAX);
        let grid = |children: Vec<AnyElement>| grid_container().pb_6().children(children);
        let body: AnyElement = match &page.items {
            SeeAllItems::Songs(songs) => {
                self.song_table(t, "see-all-songs", songs.as_ref().clone(), cx).into_any_element()
            }
            SeeAllItems::Albums(albums) => grid(
                albums
                    .iter()
                    .enumerate()
                    .map(|(i, a)| self.album_tile(t, "see-all-album", i, a, tile, cx).into_any_element())
                    .collect(),
            )
            .into_any_element(),
            SeeAllItems::Playlists(playlists) => grid(
                playlists
                    .iter()
                    .enumerate()
                    .map(|(i, p)| self.playlist_tile(t, "see-all-playlist", i, p, tile, cx).into_any_element())
                    .collect(),
            )
            .into_any_element(),
            SeeAllItems::Artists(artists) => grid(
                artists
                    .iter()
                    .enumerate()
                    .map(|(i, artist)| {
                        let id = artist.id.clone();
                        div()
                            .id(("see-all-artist", i))
                            .w(px(tile))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .child(self.avatar(&artist.artwork, &artist.name, tile, t, cx))
                            .child(div().max_w_full().truncate().child(artist.name.clone()))
                            .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Artist(id.clone()), cx)))
                            .into_any_element()
                    })
                    .collect(),
            )
            .into_any_element(),
            SeeAllItems::Stations(stations) => grid(
                stations
                    .iter()
                    .enumerate()
                    .map(|(i, station)| {
                        let url = station.url.clone();
                        div()
                            .id(("see-all-station", i))
                            .w(px(tile))
                            .flex()
                            .flex_col()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| this.open_url(&url, cx)))
                            .child(self.art(&station.artwork, tile, 8., t, cx))
                            .child(div().pt_2().truncate().child(station.name.clone()))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(t.text_secondary)
                                    .line_clamp(2)
                                    .child(station.description.clone()),
                            )
                            .into_any_element()
                    })
                    .collect(),
            )
            .into_any_element(),
            SeeAllItems::Mixed(items) => {
                let songs: Arc<Vec<Song>> = Arc::new(
                    items
                        .iter()
                        .filter_map(|i| match i {
                            crate::model::TopResult::Song(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect(),
                );
                grid(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| {
                            self.mixed_card(t, "see-all-mixed", i, item, &songs, tile, cx).into_any_element()
                        })
                        .collect(),
                )
                .into_any_element()
            }
        };
        let header = tahoe::PageHeader::new(page.title.clone()).inset(PAD).theme(*t);
        let header =
            if self.history.is_empty() { header } else { header.on_back(cx.listener(|this, _, _, cx| this.back(cx))) };
        div().child(header).child(body).into_any_element()
    }
}
