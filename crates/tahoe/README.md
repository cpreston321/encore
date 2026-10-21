# Tahoe

macOS 26 Tahoe-style components for [GPUI](https://www.gpui.rs), extracted from [Encore](../../README.md) so other GPUI apps can use them too. It's to Encore what Zed's `ui` crate is to Zed.

```sh
cargo run -p tahoe --example gallery    # every component, in your system appearance
TAHOE_APPEARANCE=light cargo run -p tahoe --example gallery
```

## What's inside

| Module | What it gives you |
| --- | --- |
| `components` | `Button`, `IconButton`, `Link`, `SectionHeader`, `PageHeader`, `Card`, `Badge`, as Zed-style builders |
| `primitives` | The same looks as plain, chainable gpui elements (`pill_button`, `circle_button`, `glass_button`, `toolbar_button`…) |
| `textfield` | `TextField`, a single-line editing model, and `TextInput`, which draws it and takes text from macOS input methods (IME composition, dead keys, press-and-hold accents, dictation, the emoji picker) for any view that implements `TextFieldHost` |
| `theme` | Light and dark color tokens, `Palette`s for surfaces tinted by artwork, and WCAG `contrast` / `readable_on` |
| `typography` | `text_shadow_on_art()` for white text on artwork |
| `anim` | `Presence` (open/close transitions that reverse mid-way), `ColorBlend`, and `reduce_motion()` |
| `icons` | The embedded icon set as an `AssetSource` (`tahoe::Assets`) and `icon(name, size, color)` |
| `chrome` | `use_tahoe_corners`: Tahoe's larger window corner radius, which macOS only gives windows with a toolbar |
| `layout` | Tile grids, `lock_scroll_axis`, `overlay_scrollbar` in theme colors, and `init_scrolling` (rubber-banding off under Reduce Motion) |

## Using it

```rust
use tahoe::prelude::*;

Application::new().with_assets(tahoe::Assets).run(|cx| {
    cx.open_window(WindowOptions::default(), |window, cx| {
        tahoe::chrome::use_tahoe_corners(window);
        cx.new(|_| Library::default())
    })
    .unwrap();
});

// In a view's render:
div()
    .child(PageHeader::new("Albums").on_back(cx.listener(|this, _, _, cx| this.back(cx))))
    .child(SectionHeader::new("recent", "Recently Added").on_click(cx.listener(|this, _, _, cx| this.open_recent(cx))))
    .child(Button::new("play", "Play").icon("play").on_click(cx.listener(|this, _, _, cx| this.play(cx))))
    .child(Link::new("artist", "Zach Bryan").on_click(cx.listener(|this, _, _, cx| this.open_artist(cx))))
```

Components pick the light or dark theme from their window. On a surface tinted by artwork, pass that surface's theme: `Button::new(..).theme(theme.tinted(palette))`. Anything drawn on a fill goes through `readable_on`, so a play button on a pale accent gets a dark icon instead of disappearing.

## Things worth knowing

- **Text hover styles.** gpui applies `.hover()` styles at paint time, after text is laid out, so an underline or text color set there never reaches the glyphs. `Link` keeps its hover state per element (`window.use_keyed_state`) and styles itself while rendering instead.
- **Rounded clipping.** `overflow_hidden` on a rounded element clips its children to the rounded shape only with the patched gpui in `vendor/gpui` (see its `PATCHES.md`). Stock gpui clips to the rectangle.
- **Tracking, shadows and clamped ellipses.** `.tracking(..)`, `.text_shadow(..)`, and an ellipsis that ends the last line of `line_clamp` text exactly, need the patched gpui too.
- **Overrides for testing.** `theme::force_appearance` and `anim::force_reduce_motion` let an app offer flags like Encore's `ENCORE_APPEARANCE` and `ENCORE_REDUCE_MOTION`.

## License

[MIT](../../LICENSE).
