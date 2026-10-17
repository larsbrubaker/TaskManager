//! The native side of the theme: the window's own appearance (its title bar, on macOS and
//! Windows) follows the app's theme, and for Match System the OS light / dark appearance is
//! read from winit's `Window::theme` and handed to the core (`AppState::set_system_dark`).
//!
//! winit reports the appearance on macOS (the app's effective appearance) and Windows (the
//! system setting); on X11 it reports nothing and on Wayland only an override, so there
//! Match System stays light. An OS appearance change wakes the event loop (winit's
//! `ThemeChanged`), and the next idle pass ([`WindowTheme::sync`]) picks it up.
//!
//! The window's appearance is set only when the choice changes: Light and Dark pin it, and
//! Match System clears the pin so `Window::theme` reports the OS again.
//!
//! While Light or Dark is pinned, macOS's `Window::theme` reports the pin, not the OS (it
//! reads the window's own appearance first), and the OS's `ThemeChanged` does not reach the
//! window, so the OS appearance the core holds goes stale. Rather than read it while pinned
//! (there is no portable source), [`WindowTheme::new`] installs a probe in the core
//! (`AppState::set_system_appearance_probe`) that releases the pin and reads
//! `Window::theme` at once. The core calls it the moment the choice goes back to Match
//! System, so the real appearance is installed immediately, in the same step, with no
//! stale frame. Releasing the pin there is what [`WindowTheme::sync`] would do next anyway.
//! (On Windows `Window::theme` always reports the OS, so the probe just reads it.)

use std::sync::Arc;

use agg_gui_shell::winit::window::{Theme as WinitTheme, Window};
use taskmanager_core::{AppState, ThemeChoice};

/// Keeps the window and the OS appearance in step with the app's theme.
pub struct WindowTheme {
    window: Arc<Window>,
    /// The choice the window's appearance was last set for.
    applied: Option<ThemeChoice>,
}

impl WindowTheme {
    /// Keep `window` in step with `state`'s theme, and give `state` the probe that reads
    /// the OS appearance on demand (see the module docs). Call [`WindowTheme::sync`] once
    /// before the first frame and on every idle pass.
    pub fn new(window: Arc<Window>, state: &AppState) -> Self {
        let probe_window = Arc::clone(&window);
        state.set_system_appearance_probe(move || {
            probe_window.set_theme(None);
            os_dark(&probe_window)
        });
        Self {
            window,
            applied: None,
        }
    }

    /// Pin or release the window's appearance if the choice changed, then report the OS
    /// appearance while the choice is Match System. Cheap enough for every idle pass.
    pub fn sync(&mut self, state: &AppState) {
        let choice = state.theme();
        if self.applied != Some(choice) {
            self.window.set_theme(pinned(choice));
            self.applied = Some(choice);
        }
        if choice == ThemeChoice::System {
            state.set_system_dark(os_dark(&self.window));
        }
    }
}

/// The OS appearance as `window` reports it; only the OS's while the window is not pinned.
fn os_dark(window: &Window) -> Option<bool> {
    window.theme().map(|t| t == WinitTheme::Dark)
}

/// The window appearance for a choice: pinned for Light and Dark, the OS's for System.
fn pinned(choice: ThemeChoice) -> Option<WinitTheme> {
    match choice {
        ThemeChoice::Light => Some(WinitTheme::Light),
        ThemeChoice::Dark => Some(WinitTheme::Dark),
        ThemeChoice::System => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_match_system_leaves_the_window_to_the_os() {
        assert_eq!(pinned(ThemeChoice::Light), Some(WinitTheme::Light));
        assert_eq!(pinned(ThemeChoice::Dark), Some(WinitTheme::Dark));
        assert_eq!(pinned(ThemeChoice::System), None);
    }
}
