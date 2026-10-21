//! Force Touch trackpad feedback, as AppKit controls give it: a light tap as something snaps
//! into place or reaches a limit. Does nothing on trackpads without Force Touch.

use objc2::{msg_send, runtime::AnyClass};

/// Which feel to give, matching `NSHapticFeedbackPattern`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    Generic = 0,
    /// Something lined up or crossed a threshold.
    Alignment = 1,
    /// A value reached a step or a limit.
    LevelChange = 2,
}

pub fn perform(haptic: Haptic) {
    let Some(manager) = AnyClass::get(c"NSHapticFeedbackManager") else { return };
    unsafe {
        let performer: *mut objc2::runtime::AnyObject = msg_send![manager, defaultPerformer];
        if performer.is_null() {
            return;
        }
        // NSHapticFeedbackPerformanceTimeNow
        let _: () = msg_send![performer, performFeedbackPattern: haptic as isize, performanceTime: 1usize];
    }
}
