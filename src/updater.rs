//! Sparkle auto-update (macOS .app builds only).
//!
//! The framework is linked when `vendor/sparkle/Sparkle.framework` is present
//! (see build.rs and scripts/fetch-sparkle.sh); every other target gets the
//! no-op stubs below. Feed URL and EdDSA public key live in macos/Info.plist;
//! release signing is scripts/bundle-macos.sh + GitHub Actions.

#[cfg(all(target_os = "macos", feature = "sparkle"))]
mod imp {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ptr;
    use std::sync::OnceLock;

    /// The SPUStandardUpdaterController, retained for the app's lifetime.
    /// Stored as a raw pointer; it is only ever touched on the main thread.
    static CONTROLLER: OnceLock<usize> = OnceLock::new();

    fn running_bundled() -> bool {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS"))
            .unwrap_or(false)
    }

    /// Instantiate the controller (which starts the updater and schedules
    /// automatic checks). Only inside a .app — a bare binary has no
    /// Info.plist, so no feed URL or key.
    pub fn start() {
        if !running_bundled() {
            return;
        }
        CONTROLLER.get_or_init(|| unsafe {
            let Some(cls) = AnyClass::get(c"SPUStandardUpdaterController") else {
                eprintln!("rista: Sparkle.framework not linked; updates disabled");
                return 0;
            };
            let ctrl: *mut AnyObject = msg_send![cls, alloc];
            let ctrl: *mut AnyObject = msg_send![
                ctrl,
                initWithUpdaterDelegate: ptr::null::<AnyObject>(),
                userDriverDelegate: ptr::null::<AnyObject>()
            ];
            ctrl as usize
        });
    }

    /// "Check for Updates…" — Sparkle's standard UI driver shows the result.
    pub fn check_for_updates() {
        let Some(&ctrl) = CONTROLLER.get() else {
            return;
        };
        if ctrl == 0 {
            return;
        }
        unsafe {
            let _: () =
                msg_send![ctrl as *mut AnyObject, checkForUpdates: ptr::null::<AnyObject>()];
        }
    }
}

#[cfg(not(all(target_os = "macos", feature = "sparkle")))]
mod imp {
    pub fn start() {}
    pub fn check_for_updates() {}
}

/// Whether this build can offer updates — used to hide the menu item on
/// builds where Sparkle was not linked.
pub const AVAILABLE: bool = cfg!(all(target_os = "macos", feature = "sparkle"));

pub use imp::{check_for_updates, start};
