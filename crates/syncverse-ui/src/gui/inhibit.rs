//! System display sleep and screen saver idle inhibitor.
//!
//! Prevents Wayland/X11 desktop screen dimming, lockouts, and DPMS sleep during video playback.
//! On Linux (Fedora Wayland/X11), communicates with `org.freedesktop.ScreenSaver` on the session bus.
//! On non-Linux platforms, provides a safe zero-cost fallback.

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::Mutex;

/// Cross-platform system idle inhibitor managing screen-saver suspension during playback.
pub struct IdleInhibitor {
    is_active: AtomicBool,
    #[cfg(target_os = "linux")]
    cookie: Mutex<Option<(u32, zbus::blocking::Connection)>>,
}

impl Default for IdleInhibitor {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleInhibitor {
    /// Creates a new uninhibited `IdleInhibitor`.
    pub fn new() -> Self {
        Self {
            is_active: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            cookie: Mutex::new(None),
        }
    }

    /// Requests idle inhibition if playback is active.
    pub fn inhibit(&self) {
        if self.is_active.swap(true, Ordering::SeqCst) {
            return;
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(mut lock) = self.cookie.lock() {
                if lock.is_none() {
                    if let Ok(conn) = zbus::blocking::Connection::session() {
                        let res: Result<u32, _> = conn
                            .call_method(
                                Some("org.freedesktop.ScreenSaver"),
                                "/org/freedesktop/ScreenSaver",
                                Some("org.freedesktop.ScreenSaver"),
                                "Inhibit",
                                &("Syncverse", "Video Playback"),
                            )
                            .and_then(|reply| reply.body().deserialize());

                        if let Ok(cookie_val) = res {
                            *lock = Some((cookie_val, conn));
                        }
                    }
                }
            }
        }
    }

    /// Releases idle inhibition when playback is paused or stopped.
    pub fn uninhibit(&self) {
        if !self.is_active.swap(false, Ordering::SeqCst) {
            return;
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(mut lock) = self.cookie.lock() {
                if let Some((cookie_val, conn)) = lock.take() {
                    let _ = conn.call_method(
                        Some("org.freedesktop.ScreenSaver"),
                        "/org/freedesktop/ScreenSaver",
                        Some("org.freedesktop.ScreenSaver"),
                        "UnInhibit",
                        &(cookie_val,),
                    );
                }
            }
        }
    }

    /// Returns whether idle inhibition is currently active.
    pub fn is_inhibited(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }
}

impl Drop for IdleInhibitor {
    fn drop(&mut self) {
        self.uninhibit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_inhibitor_lifecycle() {
        let inhibitor = IdleInhibitor::new();
        assert!(!inhibitor.is_inhibited());

        inhibitor.inhibit();
        assert!(inhibitor.is_inhibited());

        // Idempotent inhibit
        inhibitor.inhibit();
        assert!(inhibitor.is_inhibited());

        inhibitor.uninhibit();
        assert!(!inhibitor.is_inhibited());

        // Idempotent uninhibit
        inhibitor.uninhibit();
        assert!(!inhibitor.is_inhibited());
    }
}
