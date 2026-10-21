# Contributing to Encore

Thanks for your interest! Bug reports, ideas and pull requests are all welcome.

## Getting set up

```sh
git clone https://github.com/cpreston321/encore.git
cd encore
cargo run
```

That runs Encore on Apple's public APIs, which is enough for most UI work. To work on sign-in, the library or full-length playback, set up a MusicKit key as described in [docs/musickit.md](docs/musickit.md).

[docs/architecture.md](docs/architecture.md) explains how the code is organized. Read it first: it'll tell you where a change belongs.

## Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

CI runs the same three checks on macOS.

For UI changes, include a screenshot or a short recording. If the change looks different in light mode, include both appearances, and check it with Reduce Motion on.

## Debugging tools

| Variable | Effect |
| --- | --- |
| `ENCORE_PERF=1` | Logs startup milestones, each request's duration, and per-page frame times to stderr |
| `ENCORE_APPEARANCE=light` / `dark` | Forces a theme without changing the system setting |
| `ENCORE_REDUCE_MOTION=1` | Behaves as if Reduce Motion is on |
| `ENCORE_TOUR=1` | Steps through every page every few seconds, for screenshots and profiling (debug builds, or `--features tour`) |
| `ENCORE_TOUR_ONLY=albums,artist` | Limits the tour to those steps |
| `ENCORE_TOUR_PLAY=1` / `album` | Starts silent playback during the tour, of the charts or the top new album |
| `ENCORE_TOUR_EVICT_FAST=1` | Frees unused artwork after 15 seconds instead of two minutes |
| `ENCORE_TOUR_VOLUME=0.3` | Once playing, changes the volume the way the slider does |
| `ENCORE_TOUR_SEEK_END=2` | Jumps near the end of that many songs in a row, to exercise crossfades |
| `ENCORE_PLAYER=web` | Uses the web player for full songs instead of the native one |
| `ENCORE_TOUR_OVERLAY=palette` / `shortcuts` / `nowplaying` | Opens that overlay for the tour (`nowplaying` also needs `ENCORE_TOUR_PLAY=1`) |
| `ENCORE_TOUR_SCROLL=1` | On each tour page, scrolls smoothly down and back, redrawing only the page, as a real scroll does. Use it with `ENCORE_PERF=1` to check that frames fit 120 Hz |
| `ENCORE_TOUR_PLACEHOLDER_ART=1` | Replaces all artwork with generated gradients, for screenshots that don't include album covers or artist photos |

To profile an optimized build (release builds are stripped, so turn that off for `sample` or Instruments to show function names):

```sh
CARGO_PROFILE_RELEASE_STRIP=false ENCORE_PERF=1 ENCORE_TOUR=1 cargo run --release --features tour
```

`DIST=1 ./scripts/build.sh` builds the `dist` profile: fat LTO with one codegen unit, about 15% smaller again, but slow to rebuild. Use it for builds you hand to other people.

Encore targets 120 Hz on ProMotion displays: a page that renders smoothly shows a 95th percentile frame time under 8.3 ms and animates at about 120 fps.

## Code style

- `rustfmt.toml` sets a 120-column width. Let `cargo fmt` handle formatting.
- Keep comments sparse. A comment should explain what the code can't: a constraint, a workaround, or why the obvious approach doesn't work. It shouldn't narrate what the next line does.
- Reusable, app-agnostic components go in the Tahoe library (`crates/tahoe`), the way Zed keeps its `ui` crate; `cargo run -p tahoe --example gallery` shows them all. Encore-specific views stay in `ui/`.
- `ui/` code renders and dispatches. It shouldn't do I/O directly; put that in `app/` or the module that owns it.
- Anything that blocks, including every `api` call, runs on the background executor.
- New animations check `anim::reduce_motion()` and can be interrupted midway.

## Security

Never commit a MusicKit `.p8` key, `musickit.json`, a developer token or a Music User Token. To report a security issue, follow [SECURITY.md](SECURITY.md) rather than opening a public issue.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
