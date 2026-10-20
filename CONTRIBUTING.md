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
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

CI runs the same three checks on macOS.

For UI changes, include a screenshot or a short recording. If the change looks different in light mode, include both appearances, and check it with Reduce Motion on.

## Debugging tools

| Variable | Effect |
| --- | --- |
| `MUSIC_PERF=1` | Logs startup milestones, each request's duration, and per-page frame times to stderr |
| `MUSIC_APPEARANCE=light` / `dark` | Forces a theme without changing the system setting |
| `MUSIC_REDUCE_MOTION=1` | Behaves as if Reduce Motion is on |
| `MUSIC_TOUR=1` | Steps through every page every few seconds, for screenshots and profiling (debug builds, or `--features tour`) |
| `MUSIC_TOUR_ONLY=albums,artist` | Limits the tour to those steps |
| `MUSIC_TOUR_PLAY=1` | Starts silent playback during the tour |
| `MUSIC_TOUR_OVERLAY=palette` / `shortcuts` / `nowplaying` | Opens that overlay for the tour (`nowplaying` also needs `MUSIC_TOUR_PLAY=1`) |

To profile an optimized build:

```sh
MUSIC_PERF=1 MUSIC_TOUR=1 cargo run --release --features tour
```

A page that renders smoothly shows a 95th percentile frame time under 16.7 ms, with few or no slow frames.

## Code style

- `rustfmt.toml` sets a 120-column width. Let `cargo fmt` handle formatting.
- Keep comments sparse. A comment should explain what the code can't: a constraint, a workaround, or why the obvious approach doesn't work. It shouldn't narrate what the next line does.
- `ui/` code renders and dispatches. It shouldn't do I/O directly; put that in `app/` or the module that owns it.
- Anything that blocks, including every `api` call, runs on the background executor.
- New animations check `anim::reduce_motion()` and can be interrupted midway.

## Security

Never commit a MusicKit `.p8` key, `musickit.json`, a developer token or a Music User Token. If you find a security issue, please report it privately via a [GitHub security advisory](https://github.com/cpreston321/encore/security/advisories/new) instead of opening a public issue.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
