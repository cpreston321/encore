<div align="center">

<img src="docs/icon.png" width="128" height="128" alt="Encore icon">

# Encore

**An Apple Music client for macOS, written in Rust with [GPUI](https://www.gpui.rs).**

Styled after the macOS Tahoe Music app, with iOS-style detail pages and full-length playback for subscribers.

[Features](#features) · [Getting started](#getting-started) · [Apple Music setup](docs/musickit.md) · [Architecture](docs/architecture.md) · [Benchmarks](docs/benchmarks.md) · [Contributing](CONTRIBUTING.md)

</div>

![Now Playing with synced lyrics](docs/screenshots/now-playing.png)

## Features

- **Your library.** Sign in with your Apple Music account to browse your Recently Added, Artists, Albums, Songs, playlists and Favorite Songs. You can also add to your library and playlists.
- **Full songs.** Subscribers hear complete tracks through Apple's native MusicKit player on macOS 14 and later, or through MusicKit JS in a hidden web view on macOS 13. Everyone else gets 30-second previews.
- **Crossfade and gapless playback.** Choose a 3, 6 or 12 second crossfade, or none. Consecutive album tracks hand off without a gap, which the native player does by itself. Since there's only one native player, a crossfade plays the incoming song on a web player while the outgoing one fades out, and the two swap each track.
- **Animate Artwork (optional).** View → Animate Artwork makes hero cards, album, playlist and artist headers, and Now Playing slowly drift and zoom, a stand-in for Apple Music's motion artwork, which the public API doesn't provide. It's off by default, pauses while Encore isn't the active app, and respects Reduce Motion.
- **Picks up where you left off.** Quit midway through a song and Encore reopens on it, paused at the same spot with the rest of the queue intact.
- **Synced lyrics.** Lyrics come from [LRCLIB](https://lrclib.net) and follow along with the song. Click a line to jump there.
- **Detail pages that take on the artwork's colors.** Album, artist and playlist pages tint to match their artwork, the header moves with parallax, and a compact title bar fades in as you scroll.
- **Quick search (⌘K).** Spotlight-style results show a top result, artists, albums, songs and playlists from both your library and the Apple Music catalog.
- **A Mac app first.** Media keys and Control Center's Now Playing work. There's a Mini Player, Dock menu controls, a menu bar item for every page, and full keyboard navigation.
- **Visualizer.** Hovering the progress bar shows a live spectrum of the song as it plays. Before volume control is on, or on the web player, it's computed from the song's preview instead.
- **Library tools.** Sort and filter your Songs, Favorites and Albums. ⌘-click or ⇧-click to select several songs, and drag songs onto a playlist or into Playing Next.
- **Fast and accessible.** Encore scrolls and animates at 120 fps on ProMotion displays. It launches in about a quarter of a second and uses less CPU than Music.app while playing ([benchmarks](docs/benchmarks.md)). It follows the system's light or dark appearance and honors Reduce Motion.

<table>
  <tr>
    <td width="58%"><img src="docs/screenshots/album.png" alt="Album page tinted from its artwork"></td>
    <td><img src="docs/screenshots/quick-search.png" alt="Quick search"></td>
  </tr>
</table>

## Getting started

### Requirements

- macOS 13 or later. The native player needs macOS 14, and its volume control needs 14.2. The design targets macOS 26 Tahoe.
- [Rust](https://rustup.rs) 1.88 or later.
- Xcode Command Line Tools: `xcode-select --install`.

### Run it

```sh
git clone https://github.com/cpreston321/encore.git
cd encore
cargo run --release
```

Encore works without any setup. It uses Apple's public charts and the iTunes Search API, and plays 30-second previews.

### Build the app bundle

```sh
./scripts/build.sh
open ~/Applications/Encore.app
```

The script builds `Encore.app` with its icon and Info.plist and installs it to `~/Applications`. If you have an Apple Development certificate, the script signs the app with it. That keeps the app's identity stable between builds, so the Keychain's **Always Allow** stays in effect. Without a certificate, the bundle is signed ad hoc.

### Sign in to Apple Music

Signing in needs a MusicKit key from an Apple Developer account. [docs/musickit.md](docs/musickit.md) walks through creating one and pointing Encore at it. Once the key is set up:

1. Click **Sign In** at the bottom of the sidebar.
2. Encore opens a one-page sign-in site on `localhost` in your browser. Your Apple ID password only goes to Apple.
3. The page hands the Music User Token back to Encore, which stores it in your Keychain.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| <kbd>Space</kbd> | Play / pause |
| <kbd>⌘</kbd> <kbd>←</kbd> / <kbd>⌘</kbd> <kbd>→</kbd> | Previous / next track |
| <kbd>⌘</kbd> <kbd>↑</kbd> / <kbd>⌘</kbd> <kbd>↓</kbd> | Volume up / down |
| <kbd>⌘</kbd> <kbd>K</kbd> or <kbd>/</kbd> | Quick search |
| <kbd>⌘</kbd> <kbd>F</kbd> | Search page |
| <kbd>⌘</kbd> <kbd>[</kbd> / <kbd>⌘</kbd> <kbd>]</kbd> | Back / forward |
| <kbd>⌘</kbd> <kbd>1</kbd> … <kbd>⌘</kbd> <kbd>9</kbd> | Home, New, Radio, Recently Added, Artists, Albums, Songs, Playlists, Favorites |
| <kbd>⌘</kbd> <kbd>L</kbd> | Go to the current song |
| <kbd>⌃</kbd> <kbd>⌘</kbd> <kbd>F</kbd> | Now Playing |
| <kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>L</kbd> | Lyrics |
| <kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>M</kbd> | Mini Player |
| <kbd>⌥</kbd> <kbd>⌘</kbd> <kbd>U</kbd> | Playing Next |
| <kbd>⌘</kbd> <kbd>R</kbd> | Refresh library |
| <kbd>↑</kbd> <kbd>↓</kbd> <kbd>Return</kbd> | Move through a list and play |
| <kbd>⌘</kbd> <kbd>/</kbd> | Show all shortcuts |

## Limitations

These limits come from Apple's APIs, not from Encore:

- **Full-length playback needs a MusicKit key and an Apple Music subscription.** Apple only streams complete songs to its own players: MusicKit's native player, or MusicKit JS in a hidden web view.
- **Volume control for the native player needs System Audio Recording permission.** MusicKit has no volume control on macOS. So the first time you change the volume or crossfade setting, Encore asks to route the player's audio through itself using a Core Audio process tap. Nothing is recorded. If you decline, the slider changes the Mac's volume instead, and track changes don't crossfade.
- **Playlists can be added to, but not edited.** The API can add songs and create playlists. It can't remove or reorder songs, and it can't delete anything from your library.
- **Lyrics come from LRCLIB.** Apple doesn't share its own lyrics with third-party apps, so some songs have none.

## Where things are stored

| What | Where |
| --- | --- |
| Music User Token | Keychain, service `Encore` |
| MusicKit key config, local library (when signed out), settings, the last queue and position | `~/Library/Application Support/Encore/` |
| Artwork, previews, cached pages and library | `~/Library/Caches/Encore/` |

To sign out, click your name in the sidebar. To reset everything, delete the two folders and the Keychain item.

## Development

```sh
cargo test                    # unit tests
cargo test -- --ignored       # also hits the live public endpoints
cargo clippy --all-targets
cargo fmt
ENCORE_PERF=1 cargo run --release   # frame times, request durations and startup milestones on stderr
```

[CONTRIBUTING.md](CONTRIBUTING.md) covers the debug tools, and [docs/architecture.md](docs/architecture.md) explains how the code is laid out.

## Tahoe components

Encore's buttons, links, headers, cards, theme and motion live in a reusable library, [`crates/tahoe`](crates/tahoe/README.md), for other GPUI apps that want to look at home on macOS Tahoe. `cargo run -p tahoe --example gallery` shows them all.

## Acknowledgements

- [GPUI](https://www.gpui.rs), the UI framework from [Zed](https://zed.dev).
- [LRCLIB](https://lrclib.net) for synced lyrics.
- [rodio](https://github.com/RustAudio/rodio) and [Symphonia](https://github.com/pdeljanov/Symphonia) for preview playback.
- [objc2](https://github.com/madsmtm/objc2) for the AppKit, WebKit and MediaPlayer bindings.

## License

[MIT](LICENSE).

Encore isn't affiliated with or endorsed by Apple. Apple Music, MusicKit and macOS are trademarks of Apple Inc.
