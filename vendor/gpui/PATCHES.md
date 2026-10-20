# Changes from gpui 0.2.2

This is the crates.io release of [gpui](https://crates.io/crates/gpui) 0.2.2 without its examples and tests, plus one change. The commit after "Vendor gpui 0.2.2 unchanged" holds the whole diff.

## The display link sleeps when there's nothing to draw

On macOS, GPUI keeps a `CVDisplayLink` running for as long as a window is visible. On a 120 Hz display that wakes the main thread 120 times a second, even while the app is idle, and it accounted for nearly all of Encore's idle CPU.

- `PlatformWindow` gains `pause_frame_requests` and `frame_request_waker`. Both default to doing nothing, so the other platforms behave as before.
- `Window` pauses frame requests after `IDLE_FRAMES_BEFORE_PAUSE` frames in a row with nothing to draw, nothing to present and no next-frame callbacks.
- Three things wake it: the window becoming dirty (`WindowInvalidator::set_dirty` / `invalidate_view`), `on_next_frame`, which `request_animation_frame` uses, and input in `dispatch_event`.
- On macOS, pausing stops the display link and suspends its dispatch source, and waking starts both again. `stop_display_link` resumes a paused link before dropping it, so the source isn't suspended twice.

Encore's idle CPU went from about 0.6% of a core to 0.02%.
