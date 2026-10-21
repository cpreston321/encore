# Changes from gpui 0.2.2

This is the crates.io release of [gpui](https://crates.io/crates/gpui) 0.2.2 without its examples and tests, plus seven changes. `git log vendor/gpui` shows each one on its own.

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

## `overflow_hidden` clips to rounded corners

Content masks only had a rectangle, so a rounded element with `overflow_hidden` let its children show square in the corners. Every layer inside a rounded card had to repeat the card's rounding, and anything slightly larger leaked.
- **The mask.** `ContentMask` now carries `corner_radii`. `Style::overflow_mask` fills them from the element's rounding, inset by its border, when both axes are hidden.
- **Nesting.** Intersecting masks keeps a corner's radius wherever the intersection keeps that corner.
- **The shaders.** Quads, shadows, underlines, glyphs and images multiply their coverage by the mask's rounded-rectangle distance in the fragment shader, which the generated wrappers around each fragment function do. Square masks skip it.
- **Not covered:** paths and video surfaces still clip to the rectangle. Only the Metal renderer supports rounded masks.

## `line_clamp` counts every line of multi-paragraph text

`shape_text` counted only the wrap points within each paragraph toward `line_clamp`, not the paragraph's first line. Text with `\n` breaks therefore showed more lines than the clamp. Once the count ran out, the remaining paragraphs were laid out with no lines left, so they didn't wrap and were cut at the element's edge. Each paragraph now counts as one line plus its wraps, and paragraphs past the clamp aren't laid out.

## Backdrop blur

`Styled::backdrop_blur(radius)` (or `Window::paint_backdrop_blur`) blurs what has already been drawn behind an element, clipped to its rounded corners, for frosted-glass bars and panels. Pair it with a translucent background.
- **The primitive.** `BackdropBlur` carries the shape, its corner radii and a larger area it reads from. That larger area decides its draw order, so everything under it is drawn first. It sorts ahead of other primitives at the same order.
- **Metal.** The layer is no longer framebuffer-only, so the renderer can read the drawable back. For each blur it ends the render pass, copies the area into a scratch texture, shrinks it (by 4 for wide blurs), blurs it with two Gaussian passes, and draws it back inside the shape with a saturation boost (1.8, like Apple's materials). The scratch textures are kept between frames and only grow.
- **Elsewhere** the Blade and DirectX renderers skip it.

## Images drawn small use a downscaled copy

GPUI sampled every image at full resolution with no mipmaps, so artwork drawn at a fraction of its pixel size looked grainy and shimmered as it moved. The atlas also kept the full-size texture of every image ever drawn until something called `drop_image`.
- **Copies.** `img()` now paints through `Window::paint_image_downscaled`. When the image covers fewer pixels than its copy one level down, it draws the smallest copy that is still at least as large as what's on screen. Levels are 1.5 times smaller each, so a copy is never sampled at more than 1.5 texels per pixel. `texture_crop` and `motion` count toward the size (motion at its most zoomed-in), so a copy is never magnified.
- **In the background.** The first paint that needs a level starts a Catmull-Rom downscale on the background executor and draws the full image, or a larger copy that's ready, in the meantime. When it finishes, the views that asked for it are notified. `RenderImage` keeps the copies' pixels, so they go when it does.
- **Freeing tiles.** After each frame, the window frees the atlas tiles of images that frame doesn't draw: at once if it draws another copy of the same image, otherwise after `IMAGE_TILE_GRACE` (2 s). A full-size texture goes as soon as its copy is on screen. Only tiles missing from the rendered scene are freed, because that scene is the one that's presented, re-presented for motion, and replayed by cached views. An image that comes back is uploaded again from the CPU copy.
- **The Metal atlas** now frees a removed tile's space and drops the key every time. Before, it kept the key until the whole texture was empty, and removing the same key twice could free a texture that other tiles still used.
- **Other paths.** `drop_image` also removes the copies. `paint_image` and `paint_image_cropped` always draw the full image, though their tiles are freed the same way. Animated images (more than one frame) always draw their full frames, and their tiles stay until `drop_image`.
