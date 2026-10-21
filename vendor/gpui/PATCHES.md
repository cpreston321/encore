# Changes from gpui 0.2.2

This is the crates.io release of [gpui](https://crates.io/crates/gpui) 0.2.2 without its examples and tests, plus eighteen changes. `git log vendor/gpui` shows when each one landed.

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
- **Metal.** The layer is no longer framebuffer-only, so the renderer can read the drawable back. For each blur it ends the render pass, copies the area into a scratch texture, shrinks it (by 4 for wide blurs) into the blur's own texture, blurs it with two Gaussian passes, and draws it back inside the shape with a saturation boost (1.8, like Apple's materials). The scratch textures are kept between frames and only grow.
- **The background** of an element with a solid `bg` is drawn on macOS by the blur itself, over the blurred backdrop, so the glass rim below can sit on top of it. The blur follows the element's opacity, so glass fades in with its popover.
- **Reuse.** `Scene::backdrop_signatures` hashes, for each blur, every primitive drawn before it whose clipped bounds meet its sample area (shadows grown by their blur), and the signatures of earlier blurs there. The renderer keeps each blur's texture with that signature and its geometry (the copied rectangle, the drawable size, the radius), and the next frame skips the copy and both passes when they match. Anything that moves, including a scroll replay or a window resize, changes the bounds and so the signature. Sprites with `motion` and video surfaces animate without the scene changing, so a blur over one, or over a blur over one, always draws again. Textures of blurs that went away are kept as spares (up to four) for new ones. `FrameTimings` reports `blurs_reused` and `blurs_drawn`.
- **Elsewhere** the Blade and DirectX renderers skip it.

## Liquid Glass

`Styled::glass()` (or `glass_style(GlassStyle { blur, tint, rim, refraction, saturation, brightness, fallback })`, or `Window::paint_glass`) makes an element macOS 26's Liquid Glass: a backdrop blur plus what the composite fragment shader (`backdrop_blur_fragment`) adds from the shape's signed distance, with no extra passes. `backdrop_blur(radius)` is now `GlassStyle::frosted(radius)`, which adds none of it.
- **The normal** is the SDF's gradient, from four extra `quad_sdf` samples.
- **Refraction.** Within `refraction` of the edge (12 px by default, at most half the shape), the backdrop is sampled from further in, by up to 0.6 × the band, growing with the square of the closeness to the edge, so content looks magnified and pulled toward the rim like thick glass.
- **Color.** Saturation (1.6) and brightness (1.04) lift the blur, then the tint (`tint`, or the element's solid background) is laid over it.
- **The rim.** A 1.25 pt band along the edge brightens toward white, strongest where the edge faces the light from the top left and fading around the shape, with a weak reflection on the opposite side; a 6 pt band inside that opposite edge is shaded slightly darker. A faint sheen follows the lit side of the refraction band.
- **Accessibility.** `DisplayAccessibility::current()` reads `NSWorkspace`'s `accessibilityDisplayShouldReduceTransparency` and `…IncreaseContrast` once; `DisplayAccessibility::set_override` replaces them for testing. With Reduce Transparency, `Style::paint` draws no blur and fills the element with `fallback`, or else its background (or tint) made opaque. With Increase Contrast, glass draws no rim or refraction, and the element's border is drawn fully opaque. Other platforms report neither.
- **Not covered:** a settings change applies to elements painted after the next `DisplayAccessibility::current()` read, which is only the first, so the app needs a restart; and the Blade and DirectX renderers skip glass like any blur (off macOS the background stays an ordinary quad, but a `tint` isn't drawn).

## Images drawn small use a downscaled copy

GPUI sampled every image at full resolution with no mipmaps, so artwork drawn at a fraction of its pixel size looked grainy and shimmered as it moved. The atlas also kept the full-size texture of every image ever drawn until something called `drop_image`.
- **Copies.** `img()` now paints through `Window::paint_image_downscaled`. When the image covers fewer pixels than its copy one level down, it draws the smallest copy that is still at least as large as what's on screen. Levels are 1.5 times smaller each, so a copy is never sampled at more than 1.5 texels per pixel. `texture_crop` and `motion` count toward the size (motion at its most zoomed-in), so a copy is never magnified.
- **In the background.** The first paint that needs a level starts a Catmull-Rom downscale on the background executor and draws the full image, or a larger copy that's ready, in the meantime. When it finishes, the views that asked for it are notified. `RenderImage` keeps the copies' pixels, so they go when it does.
- **Freeing tiles.** After each frame, the window frees the atlas tiles of images that frame doesn't draw: at once if it draws another copy of the same image, otherwise after `IMAGE_TILE_GRACE` (2 s). A full-size texture goes as soon as its copy is on screen. Only tiles missing from the rendered scene are freed, because that scene is the one that's presented, re-presented for motion, and replayed by cached views. An image that comes back is uploaded again from the CPU copy.
- **The Metal atlas** now frees a removed tile's space and drops the key every time. Before, it kept the key until the whole texture was empty, and removing the same key twice could free a texture that other tiles still used.
- **Other paths.** `drop_image` also removes the copies. `paint_image` and `paint_image_cropped` always draw the full image, though their tiles are freed the same way. Animated images (more than one frame) always draw their full frames, and their tiles stay until `drop_image`.

## Rubber-band overscroll

With a trackpad, scrolling past either end of a scroll view stretches it with resistance and springs back when the gesture or its momentum ends, as in AppKit. Wheels with line deltas still stop at the edges.
- **Events.** `ScrollWheelEvent` gains `momentum_phase`, `None` unless the event is momentum after the fingers lift. On macOS it comes from `NSEvent.momentumPhase`, and a cancelled touch (fingers resting and lifting without moving) now ends with `TouchPhase::Ended` rather than `Moved`. The other platforms always send `None`.
- **The stretch.** Past an edge, the content is drawn `(1 - 1 / (x * 0.55 / d + 1)) * d` beyond it, where `x` is how far the fingers have moved past the edge and `d` is the view's length (`rubber_band`). Scrolling back takes up the stretch before the content moves.
- **Momentum** that reaches an edge bounces: its velocity starts the spring from the edge, capped so it goes at most a tenth of the view past it, and the rest of that fling is ignored.
- **The spring** is critically damped (`Spring`, ω = 26) and stepped exactly, so it settles in about 0.3 s at any frame rate and never crosses the edge. Touching the trackpad mid-spring catches the content where it is. Wheels with precise deltas but no phases are released 100 ms after their last event.
- **Where.** Divs with `overflow_*_scroll` (and so `uniform_list`) keep the state beside their scroll offset, in the element state or the `ScrollHandle`; `list` keeps its own. The offset stays clamped, so `ScrollHandle::offset` and everything that reads it are unchanged; the stretch is added when the children are positioned, and `ScrollHandle::overscroll` reports it. `ScrollHandle::set_offset` cancels it. Only axes that scroll and have somewhere to scroll stretch, after `restrict_scroll_to_axis` has picked the axis. A view that's stretched lets go when a gesture begins or ends over another view.
- **Frames.** The spring asks for the next frame from prepaint only while it moves, so the display link still sleeps once it settles.
- **Turning it off.** `cx.set_global(ElasticScrolling(false))` restores hard stops; Tahoe's `layout::init_scrolling` does that when Reduce Motion is on.

- **One axis per gesture.** A trackpad gesture keeps the axis its first movement favored, through its momentum, until the next gesture begins, as in AppKit. Each scroll view then drops movement on the other axis, unless it allows concurrent scrolling. Without this, the sideways drift in a vertical swipe scrolled any horizontal list under the pointer. Wheels aren't affected.

## Overlay scrollbars

`InteractiveElement::overlay_scrollbar()` (or `overlay_scrollbar_style(OverlayScrollbar { color, insets })`) draws a macOS-style scrollbar over a scroll view's edge, like "Show scroll bars: When scrolling". It's opt-in per element.
- **Showing.** A thin rounded thumb appears when the user scrolls, stays for a second after the last scroll, then fades over a quarter second. Programmatic `set_offset` doesn't show it. Moving the mouse onto the edge shows it too, and the thumb widens from 6 to 10 px with a faint track behind it. Overscroll shrinks the thumb against its end.
- **Dragging** the thumb scrolls; clicking the track jumps the thumb there and keeps dragging. While it's showing, a `BlockMouseExceptScroll` hitbox over the track keeps clicks from reaching the content underneath.
- **Colors.** By default, translucent black in light windows and white in dark ones, with a faint opposite border. `insets` keep it clear of toolbars and rounded window corners.
- **Frames.** It waits for the fade with a timer rather than drawing frames, and draws frames only while fading or widening.

## Letter spacing

`Styled::tracking(px)` sets `TextStyle::letter_spacing`, extra space after every character (negative tightens), and it cascades like the other text styles.
- **The path.** `TextRun` and `FontRun` carry it, so it's part of the line layout cache key, and runs with different spacing aren't merged. On macOS `layout_line` sets CoreText's kern attribute on each run's text (not on the ZWNJs GPUI inserts between runs), so glyph positions and the line's width include it, and wrapping, hit-testing and caret positions follow from those as before. Like CSS, the last character gets it too.
- **Single-line truncation** measures characters with `LineWrapper`, which adds the spacing to each width (`set_letter_spacing`; the pool resets it).
- **Elsewhere** the Linux and Windows text systems ignore it.

## `line_clamp` ends with a real ellipsis

With `text_overflow` and `line_clamp`, GPUI used to truncate the text to `width × lines` as if it were one long line, so the last visible line could end early or be cut mid-word. When the text wraps (`white_space: normal`), `TextLayout` now shapes it wrapped with one line more than the clamp. If that extra line exists, `line_clamp_truncation_point` finds the start of the last visible line and cuts it at the last glyph that leaves room for the suffix, dropping trailing whitespace. The suffix is measured in the style of the run it's appended to. The text is then shaped again with the suffix, and if shaping them together comes out wider and wraps, it gives up one more character and tries again. Later paragraphs count as overflow, so a clamp that ends on a paragraph's last line still gets the ellipsis. Text that doesn't wrap (`truncate()`) is truncated as before.

## Text shadows

`Styled::text_shadow(TextShadow { color, offset, blur_radius })` draws the glyphs again underneath, in the shadow's color.
- **Order.** `TextLayout::paint` paints every line's shadow (`WrappedLine::paint_shadow`) before any line's text, each in its own layer, so the text always sorts above the shadows, including the next line's. Decorations and emoji don't get shadows.
- **The blur** is rasterized once per glyph and cached in the atlas: `RenderGlyphParams` gains `blur_radius` (device pixels), and for a blurred glyph `TextSystem` rasterizes the sharp glyph, pads it by 1.5 × the radius, and runs a separable Gaussian (σ = radius / 2, as in CSS) over the coverage mask on the CPU. Blurred glyphs skip subpixel variants. Because it happens above the platform text systems, it works on every platform, and a shadow costs one extra monochrome sprite per glyph.

## VoiceOver can read and press elements

GPUI drew pixels and nothing else, so VoiceOver found an empty window.
- **Describing elements.** `InteractiveElement` gains `accessibility_label`, `accessibility_role(Role::..)`, `accessibility_value`, `accessibility_toggled`, `accessibility_selected` and `accessibility_hidden`. An element with `on_click` is a button even without them. A control without a label is named by the text painted inside it, and text outside any control becomes static text.
- **Collecting.** During paint, `Interactivity::paint` pushes a node for each such element, with its bounds clipped to the content mask, and `TextLayout::paint` adds text to it. Elements with no opacity or `accessibility_hidden` hide everything inside. A node's id hashes its `GlobalElementId` when it has one, so it stays the same object from frame to frame. Cached views replay their nodes like their other paint output. After each draw, the window hands the nodes to `PlatformWindow::update_accessibility` if they changed.
- **Cost.** Nothing is collected until the platform sends `AccessibilityRequest::Activate`, which macOS does the first time something asks the view for its children, hit-tests it or asks for its focused element. Until then each element pays one boolean check.
- **Pressing.** `AccessibilityRequest::Press` runs the node's click listeners with a keyboard `ClickEvent` carrying its bounds; text fields take focus instead. `Focus` moves GPUI focus to the node.
- **macOS.** The view is an `AXGroup` whose children are `GPUIAccessibilityElement`s, an `NSAccessibilityElement` subclass kept per node id. Each answers role, subrole, label, value, selected, focused, parent, children and frame from the latest nodes, converting window points to flipped screen coordinates when asked, so moving the window needs no update. `accessibilityPerformPress` and `setAccessibilityFocused:` go back to the window through the executor, never while GPUI is mid-frame. Updates post layout, value, title, focus and destroyed notifications.
- **The module** `accessibility` is declared before `#[macro_use] mod action`, not between them, so gpui's own tests still find `actions!`.
- **Not covered:** other platforms ignore it; sliders have no increment or decrement actions; elements that act on mouse down instead of click can't be pressed; and a window that isn't drawing (occluded, or the screen locked) doesn't update its nodes.

## Scrolling moves the last frame

A scroll only changes where a container's content sits, but GPUI notifies the view that owns the container, which renders, lays out, prepaints and paints all of the content again. `AnyView::scroll_cached(overscan, translate)` makes a view the content of a scroll container that, while only its position has changed, replays its last frame moved by the scroll, the way Core Animation moves a layer that's already drawn.
- **Moving a frame.** `Scene::replay_translated` moves every primitive in the range, with its content mask, path vertices, sprite transform and blur area. `Window::reuse_prepaint_translated` moves the hitboxes, so hit testing, hover, cursors and scroll targets follow the content. Mouse listeners carry over unchanged; drags look up their hitbox's current bounds (`Window::current_hitbox_bounds`) rather than the bounds they captured.
- **Overscan.** The view renders with its content mask grown by `overscan` on each side, so the frame it records includes what's just out of view. `Scene::push_clip` and the hitbox clip then limit what's drawn and hit-tested to the real mask. The recorded primitives keep the grown mask, and a replay intersects them with the current one, so content scrolls in from the edges already drawn.
- **When it renders instead.** When the view is notified or the window refreshes; when the move would show past the drawn area; when the mouse would be over a different set of its hitboxes after the move, so hover styles stay right; while something is dragged; when its last frame has deferred draws or input handlers, which aren't moved; when its opacity, content mask or text style changed; when its parent replayed the previous frame without visiting it; and when the caller passes `translate: false`, for frames where something drawn depends on the scroll offset. A cached parent that renders no longer forces it to render too (`refresh_from_cache_miss`).
- **Sizing.** The view lays out at the size it measured when it last rendered, so its scroll container still knows the content's height without rendering it.
- **Measuring.** `Window::frame_timings` reports where the last frame went (render, layout, prepaint, paint, encoding, and the GPU time Metal reports) and how many scroll-cached views moved or rendered.

## Color-managed, wide-gamut rendering on macOS

GPUI left the `CAMetalLayer`'s `colorspace` unset, which tells the compositor not to color-match the drawable: its values went to the display as the display's own primaries. On a wide-gamut display that made every sRGB color, UI and artwork alike, more saturated than specified. Images were decoded with the `image` crate, which ignores embedded color profiles, so Display P3 or Adobe RGB pixels were drawn as if they were sRGB numbers.
- **The drawable** is tagged extended sRGB. Colors in sRGB keep their values, and blending still happens on gamma-encoded sRGB values as before. Colors outside sRGB are components below 0 or above 1. Opaque windows on Apple GPUs use `BGR10_XR`, which is the same size as `BGRA8Unorm` and holds every 8-bit sRGB value exactly. Transparent windows and other GPUs use `RGBA16Float`, which doubles the drawable's memory and bandwidth. `update_transparency` switches format and rebuilds the drawable's pipelines, and the backdrop blur's scratch textures use the drawable's format. Path intermediates stay `BGRA8Unorm`, since they only hold sRGB colors.
- **Decoding.** Still images go through `decode_still_image`, which reads the embedded ICC profile. An image with a profile wider than sRGB is converted to Display P3 with vImage, or kept as is if it's already Display P3. Other RGB profiles are converted to sRGB, and images without a profile, or with an sRGB one, are left untouched. `RenderImage::color_space` records the result, and downscaled copies inherit it. Animated GIFs and WebPs, and SVGs, are still treated as sRGB.
- **Drawing.** `PolychromeSprite` gains `display_p3`. The Metal fragment shader converts those texels to extended sRGB (the exact sRGB transfer function, mirrored below 0, and the P3-to-sRGB matrix), so a P3 image keeps its 8-bit atlas texture. Glyphs, emoji and sRGB images skip it.
- **Backdrop blur** clamps its saturation boost to the range of the color it boosts, rather than to 0...1, so sRGB colors still can't leave sRGB but wide colors under a blur aren't clipped.
- **Reading colors.** `convert_rgba_to_srgb` converts pixels tagged with a profile to sRGB, so a color averaged from an image matches GPUI's colors.
- **Elsewhere** the Blade and DirectX renderers don't tag their surfaces. With `macos-blade`, wide-gamut images are clipped to sRGB instead; on other platforms color profiles are still ignored.

## Windows can be snapshotted offscreen

`Window::snapshot` draws the window and renders the frame into an offscreen texture, returning it as an 8-bit sRGB image (wide-gamut colors are clamped). The window draws nothing while the screen is locked or the display sleeps, and screen capture sees nothing then either; this works regardless, so tools and tests can look at the app. `PlatformWindow::snapshot` does the rendering; only macOS implements it. `DispatchEventResult` is now public, so code outside GPUI can call `Window::dispatch_event` to drive a window with synthetic input.
