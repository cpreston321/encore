//! Native window tweaks gpui doesn't expose.

use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSToolbar, NSView, NSWindowToolbarStyle};
use objc2_foundation::NSString;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// macOS Tahoe only gives windows its large, continuous corner radius when they have a
/// toolbar. An empty, invisible unified toolbar earns the window those corners, which the
/// floating sidebar's own radius is designed to be concentric with.
pub fn use_tahoe_corners(window: &gpui::Window) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: gpui hands out the NSView that backs this window; we're on the main thread.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let Some(ns_window) = view.window() else {
        return;
    };
    let toolbar: Retained<NSToolbar> =
        NSToolbar::initWithIdentifier(mtm.alloc(), &NSString::from_str("io.github.cpreston321.encore.main"));
    #[allow(deprecated)]
    toolbar.setShowsBaselineSeparator(false);
    ns_window.setToolbar(Some(&toolbar));
    ns_window.setToolbarStyle(NSWindowToolbarStyle::Unified);
}
