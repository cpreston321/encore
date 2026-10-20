# Encore

An Apple Music–style client for macOS built with [GPUI](https://www.gpui.rs), modeled on the macOS Tahoe Music app.

```sh
cargo run --release
```

To build the app itself, run `scripts/build.sh`. It produces `Encore.app` (name, icon,
Info.plist), signs it with your Apple Development certificate so the Keychain's "Always
Allow" carries over between builds, and installs it to `~/Applications`:

```sh
./scripts/build.sh && open ~/Applications/Encore.app
```

The icon is drawn by `scripts/make_icon.swift`; regenerate `assets/AppIcon.icns` from it if
you change the design.

## Data sources

Out of the box it uses Apple's public, no-key APIs:

- **Charts**: `rss.marketingtools.apple.com` (top songs, albums, playlists). These feed Home, New ("Best New Songs" are chart songs from the last ~6 weeks) and Radio.
- **Details, search, previews**: the iTunes Search/Lookup API (album tracks, artist pages, search, 30-second previews, artwork).

The public APIs can't list the songs in Apple's editorial playlists, and only the 30-second previews can be played. Apple restricts full-track playback to its own players.

### Using the official Apple Music API (MusicKit)

With a MusicKit key from a paid Apple Developer account (Certificates, Identifiers & Profiles → Keys → enable MusicKit), the app switches to `api.music.apple.com`. That adds playlist track listings, artist artwork and Apple's own charts. Configure it with any one of:

```sh
# 1. A pre-signed developer token
export APPLE_MUSIC_DEVELOPER_TOKEN=eyJ...

# 2. The key itself (the app signs an ES256 token)
export APPLE_MUSIC_TEAM_ID=ABCDE12345
export APPLE_MUSIC_KEY_ID=XYZ9876543
export APPLE_MUSIC_PRIVATE_KEY_PATH=~/keys/AuthKey_XYZ9876543.p8
```

```jsonc
// 3. ~/Library/Application Support/GPUI Music/musickit.json
{ "team_id": "ABCDE12345", "key_id": "XYZ9876543", "private_key_path": "/Users/you/keys/AuthKey_XYZ9876543.p8" }
```

The sidebar footer shows which source is active.

## Signing in to your library

With a MusicKit key configured, click **Sign In** at the bottom of the sidebar:

1. The app starts a one-page sign-in site on `localhost` and opens it in your browser.
2. **Sign In with Apple Music** runs Apple's MusicKit sign-in. Your Apple ID password only goes to Apple.
3. The page hands the resulting Music User Token back to the app, which stores it in the macOS Keychain (service "GPUI Music").

Once you're signed in, Recently Added, Artists, Albums, Songs and the Playlists section show your real Apple Music library, and **+** / **Add to Library** add items to it. Apple's API can add to a library but not remove from it. **Favorite** sets the song's Apple Music rating, and Favorite Songs lists the favorited songs in your library. Click your name in the sidebar to refresh or sign out. Playback is still limited to 30-second previews.

Without signing in, things you add or favorite with **+** or **⋯** are kept locally in `~/Library/Application Support/GPUI Music/library.json`.

Caches (artwork, previews, last charts) live in `~/Library/Caches/gpui-music/`.

## Features

- **Now Playing** (click the cover in the player, or ⌃⌘F): full-window artwork, big controls, Up Next.
- **Lyrics** (⇧⌘L): time-synced lyrics from [LRCLIB](https://lrclib.net); click a line to jump there.
  Apple doesn't share its own lyrics with third-party apps.
- **Mini Player** (⇧⌘M) and Dock menu controls; media keys and Control Center's Now Playing work.
- **Crossfade** (Playing Next panel) and gapless handoff between consecutive album tracks.
- **Playlists**: Add to Playlist / New Playlist from any song's ⋯ or right-click menu, or drag songs
  onto a playlist in the sidebar or into Playing Next. Apple's API can't remove or reorder songs.
- **Library**: sort and filter Songs, Favorites and Albums; ⌘-click / ⇧-click to select several songs.
- **Home** shows Apple's personal recommendations, Recently Played and Heavy Rotation when signed in.
- Honors the system Reduce Motion setting. `MUSIC_APPEARANCE=light|dark` forces a theme.

## Keys

| Shortcut | Action |
| --- | --- |
| Space | Play / pause |
| ⌘← / ⌘→ | Previous / next |
| ⌘↑ / ⌘↓ | Volume |
| ⌘F | Search |
| ⌘[ | Back |
| ⌘⌥U | Playing Next |

## Performance

Set `MUSIC_PERF=1` to log startup milestones, every Apple request's duration, and a frame-time
summary per page every 5 seconds (average, 95th percentile, max, frames over 16.7ms) to stderr:

```sh
MUSIC_PERF=1 cargo run --release
```

`cargo build --release --features tour` adds the automated page tour (`MUSIC_TOUR=1`) to an
optimized build, for profiling every page.

## Tests

```sh
cargo test                 # unit tests
cargo test -- --ignored    # also hits the live public endpoints
```
