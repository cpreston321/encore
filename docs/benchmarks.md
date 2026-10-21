# Benchmarks

How Encore compares with Apple's Music app on the same Mac. Both apps were signed in to the same Apple Music account, and both streamed full songs during the playing measurements.

Apple M4 Max, 64 GB, macOS 26.5.2 · Encore 0.1.0 · Music 1.6.5 · 2026-10-21

| | Encore | Music |
| --- | ---: | ---: |
| Launch to first window (warm, median of 5) | **267 ms** | 717 ms |
| Memory, idle | **223 MB** | 618 MB |
| Memory, playing | 657 MB | **393 MB** |
| CPU, idle (30 s average) | **0.0%** | 5.2% |
| CPU, playing (30 s average) | **3.9%** | 14.4% |
| Processes while playing | **3** | 6 |
| Size on disk | **18 MB** | 67 MB |

Playing, about 390 MB of Encore's total is buffers macOS keeps for a window while it presents frames (the progress bar redraws a few times a second). They're charged to Encore but aren't its allocations: its renderer holds about 75 MB of GPU memory. They go as soon as the window stops drawing (paused, minimized, or idle), and builds from before this session's changes show them too. Music's idle CPU includes it still settling after launch.

How each number is measured:
- **Memory and CPU** cover the app plus every helper process macOS holds it responsible for, the same grouping Activity Monitor uses. For Encore, that includes WebKit's content, network and GPU processes, which run MusicKit JS. For Music, it includes its XPC services.
- **CPU** is the share of one core, so 100% means one core fully busy.
- **Idle** means 20 seconds after launch, on the page the app opened to. At that point Encore may still be loading the library and artwork.
- **Size on disk** is just the app bundle. Music also relies on system frameworks that aren't counted.

## Frame rate

The built-in Liquid Retina XDR display refreshes at up to 120 Hz, which leaves 8.3 ms per frame. These numbers come from scrolling each page continuously with `ENCORE_TOUR_SCROLL=1 ENCORE_TOUR_SCROLL_SECS=8` and `ENCORE_PERF=1`, after the first seconds of loading:

| Page | Frame rate | Average frame | p95 frame | Before scroll replay | Page content moved, not drawn | CPU while scrolling |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| New | 120 fps | 0.7 ms | 0.9 ms | 2.5 ms | 99% | 25% |
| Home | 120 fps | 1.1 ms | 1.9 ms | 4.8 ms | 99% | 33% |
| Radio | 120 fps | 1.0 ms | 2.0 ms | 2.4 ms | 83% | |

While a page only scrolls, its last frame is moved rather than drawn again ([PATCHES.md](../vendor/gpui/PATCHES.md#scrolling-moves-the-last-frame)), so layout and paint nearly disappear from scroll frames. What remains is mostly encoding GPU commands, 2.5–4 ms a frame, most of it for the sidebar's and player's glass, which has to be blurred again while the page moves under it. Frames over budget, about 1%, are pages loading or artwork arriving. Apple's Music app doesn't expose frame timings, so it has no column here.

## Native player

On macOS 14+, full songs play through MusicKit's native player instead of MusicKit JS in a web view. Here's a second run with the native player, against the web player run above:

| While playing | Encore, web player | Encore, native player | Music |
| --- | ---: | ---: | ---: |
| CPU (30 s average) | 5.5% | **2.9%** | 3.0% |
| Processes | 6 | **3** | 6 |
| Memory added by playback | +110 MB | **+61 MB** | |

Memory totals varied between the two runs: idle Encore was 280 MB in the first and 542 MB in the second, and Music varied as much. That's down to how much artwork each app had loaded, so this table shows how much playback added. A crossfade still starts a web player for the incoming song, so these numbers apply to the time between crossfades, or to all of it with crossfade off.

## Memory over a session

Encore used to keep every cover it had shown in memory until it quit. Touring 15 pages took the process to **934 MB**. It now frees artwork that hasn't been drawn for two minutes, and GPUI frees an image's GPU copy two seconds after it's last drawn and draws small artwork from a downscaled copy. The full 17-page tour (`ENCORE_TOUR=1`) now ends at **485 MB** and settles to **417 MB** 40 seconds later. Revisiting a page decodes its artwork again from the disk cache.

Album playback no longer starts a web player for every track change. On the native player, consecutive tracks play from MusicKit's own queue, so WebKit isn't running at all during an album.

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
