# Changes from gpui 0.2.2

This is the crates.io release of [gpui](https://crates.io/crates/gpui) 0.2.2 without its examples and tests, plus four changes. `git log vendor/gpui` shows each one on its own.

## The display link sleeps when there's nothing to draw

On macOS, GPUI keeps a `CVDisplayLink` running for as long as a window is visible. On a 120 Hz display that wakes the main thread 120 times a second, even while the app is idle, and it accounted for nearly all of Encore's idle CPU.

- `PlatformWindow` gains `pause_frame_requests` and `frame_request_waker`. Both default to doing nothing, so the other platforms behave as before.
- `Window` pauses frame requests after `IDLE_FRAMES_BEFORE_PAUSE` frames in a row with nothing to draw, nothing to present and no next-frame callbacks.
- Three things wake it: the window becoming dirty (`WindowInvalidator::set_dirty` / `invalidate_view`), `on_next_frame`, which `request_animation_frame` uses, and input in `dispatch_event`.
- On macOS, pausing stops the display link and suspends its dispatch source, and waking starts both again. `stop_display_link` resumes a paused link before dropping it, so the source isn't suspended twice.

Encore's idle CPU went from about 0.6% of a core to 0.02%.

## A deadlock when a window gets a spurious "became key"

When the app isn't active, Cocoa can send `windowDidBecomeKey:` to a window that isn't actually key. That happens, for example, when Encore launches while another app is in front. GPUI works around this in `window_did_change_key_status` by calling `resignKeyWindow`, but it did so while holding the window state's lock. `resignKeyWindow` posts `windowDidResignKey:` synchronously, which re-enters the same function and waits on the lock forever. The app then hangs on launch and ignores Quit. The fix releases the lock before the call.

## Images can draw part of their texture

`StyledImage::texture_crop` (and `Window::paint_image_cropped`) draws a sub-rectangle of an image, given as fractions of its width and height, stretched over the element. Polychrome sprites carry the crop, which defaults to the whole texture, and the Metal vertex shader maps texture coordinates through it. Encore uses it to slowly pan and zoom artwork inside its rounded corners. GPUI can only clip to rectangles, so drawing an enlarged image would square off the corners. Only the Metal renderer reads the crop; the Blade shaders used on Linux and Windows don't.

## Images can move on the GPU

`StyledImage::motion(phase, strength)` gives an image a slow pan and zoom that the Metal vertex shader computes from a motion clock, so the image moves without the window rendering again.
- **Presenting.** While a visible sprite has motion and the window is active, the window re-presents the last scene at `MOTION_FPS` (20), with no render, layout or paint. Between those frames it pauses the display link and wakes it with a timer, rather than waking on every vsync.
- **What counts as visible.** A sprite counts only if at least 10% of it is inside its content mask. That way a sliver left at the edge of a scroll view, usually under a header, doesn't keep the window busy. Sprites outside the mask aren't in the scene at all.
- **The motion clock** advances only while it's presenting moving sprites, so motion that stopped resumes where it was. It caps each step at 0.1 s.

Encore's Animate Artwork costs about 2% of a core this way, against 3–4% when it re-rendered the page at 10 fps. Only the Metal renderer supports it.
