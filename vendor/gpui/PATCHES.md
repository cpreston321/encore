# Changes from gpui 0.2.2

This is the crates.io release of [gpui](https://crates.io/crates/gpui) 0.2.2 without its examples and tests, plus two changes. `git log vendor/gpui` shows each one on its own.

## The display link sleeps when there's nothing to draw

On macOS, GPUI keeps a `CVDisplayLink` running for as long as a window is visible. On a 120 Hz display that wakes the main thread 120 times a second, even while the app is idle, and it accounted for nearly all of Encore's idle CPU.

- `PlatformWindow` gains `pause_frame_requests` and `frame_request_waker`. Both default to doing nothing, so the other platforms behave as before.
- `Window` pauses frame requests after `IDLE_FRAMES_BEFORE_PAUSE` frames in a row with nothing to draw, nothing to present and no next-frame callbacks.
- Three things wake it: the window becoming dirty (`WindowInvalidator::set_dirty` / `invalidate_view`), `on_next_frame`, which `request_animation_frame` uses, and input in `dispatch_event`.
- On macOS, pausing stops the display link and suspends its dispatch source, and waking starts both again. `stop_display_link` resumes a paused link before dropping it, so the source isn't suspended twice.

Encore's idle CPU went from about 0.6% of a core to 0.02%.

## A deadlock when a window gets a spurious "became key"

When the app isn't active, Cocoa can send `windowDidBecomeKey:` to a window that isn't actually key. That happens, for example, when Encore launches while another app is in front. GPUI works around this in `window_did_change_key_status` by calling `resignKeyWindow`, but it did so while holding the window state's lock. `resignKeyWindow` posts `windowDidResignKey:` synchronously, which re-enters the same function and waits on the lock forever. The app then hangs on launch and ignores Quit. The fix releases the lock before the call.
