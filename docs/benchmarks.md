# Benchmarks

How Encore compares with Apple's Music app on the same Mac. Both apps were signed in to the same Apple Music account, and both streamed full songs during the playing measurements.

Apple M4 Max, 64 GB, macOS 26.5.2 · Encore 0.1.0 · Music 1.6.5 · 2026-10-20

| | Encore | Music |
| --- | ---: | ---: |
| Launch to first window (warm, median of 5) | **267 ms** | 809 ms |
| Memory, idle | **280 MB** | 366 MB |
| Memory, playing | **390 MB** | 402 MB |
| CPU, idle (30 s average) | 1.3% | **0.2%** |
| CPU, playing (30 s average) | **5.5%** | 8.4% |
| Processes while playing | 6 | 6 |
| Size on disk | **21 MB** | 67 MB |

How each number is measured:
- **Memory and CPU** cover the app plus every helper process macOS holds it responsible for, the same grouping Activity Monitor uses. For Encore, that includes WebKit's content, network and GPU processes, which run MusicKit JS. For Music, it includes its XPC services.
- **CPU** is the share of one core, so 100% means one core fully busy.
- **Idle** means 20 seconds after launch, on the page the app opened to. At that point Encore may still be loading the library and artwork.
- **Size on disk** is just the app bundle. Music also relies on system frameworks that aren't counted.

## Frame rate

The built-in Liquid Retina XDR display refreshes at up to 120 Hz, which leaves 8.3 ms per frame. These numbers come from scrolling each page smoothly down and back with `ENCORE_TOUR_SCROLL=1` and `ENCORE_PERF=1`:

| Page | Frame rate | Average frame | p95 frame |
| --- | ---: | ---: | ---: |
| New | 120 fps | 2.5 ms | 3.7 ms |
| Home | 120 fps | 4.8 ms | 5.2 ms |
| Radio | 120 fps | 2.4 ms | 3.6 ms |
| Album | 120 fps | 2.0 ms | 3.2 ms |
| Artist | 120 fps | 3.8 ms | 7.7 ms |
| Playlist | 120 fps | 4.3 ms | 7.6 ms |

A few frames on the artist and playlist pages go over budget, about 3% of them. Those are most likely moments when newly downloaded artwork arrives mid-scroll and redraws the whole window. Apple's Music app doesn't expose frame timings, so it has no column here.

## Idle

GPUI keeps the macOS display link running for as long as a window is visible, which wakes the app 120 times a second even when nothing changes. Encore carries a small patch to GPUI ([vendor/gpui/PATCHES.md](../vendor/gpui/PATCHES.md)) that lets the link sleep until something needs drawing. Encore's own timers also run only while a song is playing. Measured on its own after settling, Encore's idle CPU went from **0.64%** to **0.02%**, against Music's 0.2%. The table above was recorded before this change.

## What made the difference

The first run of this benchmark had Encore using **16.8%** CPU while playing, which was twice Music's. Three changes brought it down to 5.5%:

- **The progress bar redraws only when it visibly moves.** It used to notify 30 times a second, and in GPUI that re-rendered the whole window, at about 6.5 ms per frame.
- **The page, player and sidebar are cached views.** A playback tick now redraws only the player.
- **The web player long-polls for commands.** It used to ask the bridge 12 times a second.

Together these took Encore's own process from 22.6% to 2.0% of a core while playing. The rest of the 5.5% is WebKit, which streams and decodes the protected audio. See [architecture.md](architecture.md#rendering-and-performance) for how the cached regions work.

## Running it yourself

```sh
./scripts/build.sh --features tour   # the playing measurement uses the tour's silent playback
./scripts/bench.sh                   # RUNS=5 SETTLE=20 SAMPLE=30 to change the defaults
```

The script quits and relaunches both apps and plays a song in each with the volume at zero. Afterwards it restores Music's volume and closes Music if it wasn't open before. Sign in to Apple Music in both apps first. Results are written to `target/bench/results.md`. Expect numbers to vary by a few percent from run to run.
