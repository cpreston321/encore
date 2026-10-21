<div align="center">

<img src="docs/icon.png" width="128" height="128" alt="Encore icon">

# Encore

**An Apple Music client for macOS, written in Rust with [GPUI](https://www.gpui.rs).**

Styled after the macOS Tahoe Music app, with iOS-style detail pages and full-length playback for subscribers.

[Features](#features) · [Screenshots](#screenshots) · [Benchmarks](#benchmarks) · [Getting started](#getting-started) · [Apple Music setup](docs/musickit.md) · [Architecture](docs/architecture.md) · [Contributing](CONTRIBUTING.md)

</div>

<p align="center">
  <img src="docs/screenshots/now-playing.png" alt="Now Playing with synced lyrics">
</p>

## Features

- **Your Apple Music library**, playlists and favorites, with full songs for subscribers
- **Synced lyrics**, crossfade and gapless playback
- **Pages that take on the artwork's colors**, with parallax headers
- **Quick search (⌘K)** across your library and the catalog
- **Mini Player (⇧⌘M)** that stands in for the main window
- **Trackpad gestures**: swipe back and forward, swipe the player to skip
- **A real Mac app**: media keys, Control Center, VoiceOver, menus for everything
- **Fast and light**: launches in a quarter of a second, 120 fps scrolling

[All features →](docs/features.md)

## Screenshots

<p align="center">
  <img src="docs/screenshots/home.png" width="48%" alt="Home, with the floating player bar">
  <img src="docs/screenshots/new.png" width="48%" alt="New, with the week's albums and songs">
</p>
<p align="center"><sub><b>Home</b> with the floating player bar &nbsp;·&nbsp; <b>New</b> this week</sub></p>

<p align="center">
  <img src="docs/screenshots/album.png" width="48%" alt="Album page tinted from its artwork">
  <img src="docs/screenshots/quick-search.png" width="48%" alt="Quick search">
</p>
<p align="center"><sub><b>Album pages</b> tinted from the artwork &nbsp;·&nbsp; <b>Quick search</b> with ⌘K</sub></p>

<p align="center">
  <img src="docs/screenshots/welcome.png" width="48%" alt="Welcome screen on a new install">
  <img src="docs/screenshots/setup.png" width="48%" alt="Connecting a MusicKit key">
</p>
<p align="center"><sub><b>Welcome</b> on a new install &nbsp;·&nbsp; <b>Guided setup</b> for your own MusicKit key</sub></p>

<p align="center">
  <img src="docs/screenshots/mini-player.png" width="260" alt="Mini Player with its controls showing">
</p>
<p align="center"><sub><b>Mini Player</b> with ⇧⌘M</sub></p>

<sub>Album artwork is replaced with stand-in gradients. [CONTRIBUTING.md](CONTRIBUTING.md) explains how to take and frame these.</sub>

## Benchmarks

Against Apple's Music app on the same Mac, both signed in and streaming full songs. Lower is better.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/benchmarks-dark.svg">
  <img src="docs/benchmarks-light.svg" width="880" alt="Encore against Music: launch 270 ms vs 848 ms; memory idle 234 MB vs 362 MB; memory playing 272 MB vs 404 MB; CPU idle 0.0% vs 0.6%; CPU playing 1.7% vs 8.7%; size on disk 18 MB vs 67 MB">
</picture>

<sub>Apple M4 Max, macOS 26.5.2, Encore 0.1.0 and Music 1.6.5. Memory and CPU include each app's helper processes; CPU is a 30-second average of one core. [Full results and how they're measured →](docs/benchmarks.md)</sub>

## Getting started

Encore runs on macOS 13 or later. The native player needs macOS 14, and its volume control needs 14.2. The design targets macOS 26 Tahoe.

### Install from source

```sh
git clone https://github.com/cpreston321/encore.git
cd encore
./scripts/install.sh
```

The script checks for the Xcode Command Line Tools and [Rust](https://rustup.rs) 1.88 or later and offers to install what's missing. It then builds `Encore.app` and installs it to `~/Applications`. An app built on your own Mac isn't quarantined, so macOS opens it without asking. To update, run `git pull && ./scripts/install.sh`.

### Install a release

Each [release](https://github.com/cpreston321/encore/releases) has a zip of `Encore.app` for Apple silicon Macs, signed ad hoc rather than with a Developer ID. Releases can sign in to Apple Music without any setup (see [Sign in to Apple Music](#sign-in-to-apple-music)).

1. Download and unzip it, and move `Encore.app` to Applications.
2. Open it. macOS says it can't check the app for malicious software.
3. Open **System Settings → Privacy & Security**, scroll down to the message about Encore, and click **Open Anyway**. On macOS 15 and later, right-clicking the app and choosing **Open** no longer skips this.

Alternatively, sign the downloaded app with your own certificate. That also stops macOS asking about it, and keeps the Keychain's **Always Allow** in effect across updates:

```sh
./scripts/sign.sh /Applications/Encore.app
```

### Signing

`scripts/sign.sh` signs with, in order:

1. `ENCORE_SIGN_IDENTITY` (a certificate's name, or `-` for ad hoc);
2. your Apple Development certificate;
3. your Developer ID Application certificate;
4. ad hoc.

`scripts/build.sh` signs every build with it. macOS remembers Keychain access per code signature, so with a certificate the Keychain stops asking after each build. Ad hoc signatures change with every build, so the prompt comes back.

### Run it while developing

```sh
cargo run --release
```

Encore works without any setup. It uses Apple's public charts and the iTunes Search API, and plays 30-second previews. `./scripts/build.sh` builds and installs the app bundle, with its icon and Info.plist.

### Sign in to Apple Music

Releases come with a MusicKit developer token built in, so you can sign in straight away. Apple limits a token to six months, so each release's token lasts 180 days; after that, update to a newer release or connect a key of your own.

Builds from source need a MusicKit key from an Apple Developer account. [docs/musickit.md](docs/musickit.md) walks through creating one and pointing Encore at it, and the app's welcome screen does the same. A key of your own always takes the place of a release's token. Then:

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

To release, bump `version` in `Cargo.toml` and push a matching tag (`v0.2.0`). The [release workflow](.github/workflows/release.yml) builds the app and attaches it to a draft release, which stays unpublished until you publish it. It needs the `MUSICKIT_TEAM_ID`, `MUSICKIT_KEY_ID` and `MUSICKIT_PRIVATE_KEY` (the `.p8` file's contents) repository secrets: it signs a 180-day developer token with them and builds it into the app, and the key itself goes no further. Release at least every six months, or sign-in stops working for people without a key of their own.

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
