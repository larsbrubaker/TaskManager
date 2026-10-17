//! The theme on the state side: the user's [`ThemeChoice`] (the sidebar footer's sun / moon
//! button, `TASKMANAGER_THEME`, later the Settings page), the OS appearance the shell
//! reports for Match System ([`AppState::set_system_dark`]), and installing the resolved
//! look.
//!
//! [`AppState::apply_theme`] installs agg-gui's `Visuals` for the look (`theme::visuals`)
//! and bumps [`AppState::theme_version`]; the chrome is built with theme colours and
//! rebuilds on it. Widgets that paint with `ctx.visuals()` follow at once.
//!
//! While Light or Dark is pinned a shell may be unable to see the OS appearance (on macOS
//! the window reports its pinned look), so the last reported one can be stale. A shell
//! that has that problem installs a probe ([`AppState::set_system_appearance_probe`]) that
//! reads the real OS appearance on demand; switching back to Match System calls it first,
//! so the look installed is the OS's at once, never the stale one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::theme::set_visuals;

use super::AppState;
use crate::theme::{self, ThemeChoice};

/// See the module docs.
#[derive(Default)]
pub(super) struct ThemeState {
    choice: Cell<ThemeChoice>,
    /// The OS appearance: `Some(true)` dark, `None` unknown.
    system_dark: Cell<Option<bool>>,
    /// The look last installed (`None` before the first [`AppState::apply_theme`]).
    applied_dark: Cell<Option<bool>>,
    version: Cell<u64>,
    /// Reads the OS appearance now (see the module docs); `None` until a shell installs one.
    probe: RefCell<Option<SystemAppearanceProbe>>,
}

/// Reads the OS appearance on demand: `Some(true)` dark, `None` when it cannot tell.
type SystemAppearanceProbe = Rc<dyn Fn() -> Option<bool>>;

impl AppState {
    fn theme_state(&self) -> &ThemeState {
        &self.inner.theme
    }

    /// The user's choice: Match System, Light or Dark.
    pub fn theme(&self) -> ThemeChoice {
        self.theme_state().choice.get()
    }

    /// Use `choice`, switching the look now if it changes it. Switching to Match System
    /// first refreshes the OS appearance through the probe, if one is installed.
    pub fn set_theme(&self, choice: ThemeChoice) {
        if self.theme() != choice {
            if choice == ThemeChoice::System {
                // Clone out of the cell: the probe may call back into the shell.
                let probe = self.theme_state().probe.borrow().clone();
                if let Some(probe) = probe {
                    self.theme_state().system_dark.set(probe());
                }
            }
            self.theme_state().choice.set(choice);
            // Installing a new look bumps `theme_version`, which rebuilds the chrome
            // (the footer button shows the look). A choice that keeps the look shows
            // nothing new yet; the Settings page will read the choice when it lands.
            self.apply_theme_if_changed();
        }
    }

    /// Install the shell's way of reading the OS appearance on demand (see the module
    /// docs). Shells that always see the OS appearance (the browser's `matchMedia`) need
    /// none: they keep [`AppState::set_system_dark`] current instead.
    pub fn set_system_appearance_probe(&self, probe: impl Fn() -> Option<bool> + 'static) {
        *self.theme_state().probe.borrow_mut() = Some(Rc::new(probe));
    }

    /// Flip between light and dark (the footer button): the opposite of the look shown.
    pub fn toggle_theme(&self) {
        self.set_theme(if self.is_dark() {
            ThemeChoice::Light
        } else {
            ThemeChoice::Dark
        });
    }

    /// The OS appearance, as the shell last reported it (`None`: it cannot tell).
    pub fn system_dark(&self) -> Option<bool> {
        self.theme_state().system_dark.get()
    }

    /// The shell reports the OS appearance (`Some(true)` dark, `None` when it cannot tell).
    /// Under Match System the look follows it at once. Cheap to call every idle pass:
    /// nothing happens unless it changed.
    pub fn set_system_dark(&self, dark: Option<bool>) {
        if self.system_dark() != dark {
            self.theme_state().system_dark.set(dark);
            self.apply_theme_if_changed();
        }
    }

    /// Whether the look is dark: the choice, resolved against the OS appearance.
    pub fn is_dark(&self) -> bool {
        self.theme().is_dark(self.system_dark())
    }

    /// Bumped each time a look is installed.
    pub fn theme_version(&self) -> u64 {
        self.theme_state().version.get()
    }

    /// Install the look's `Visuals` and tell the widgets. `build_app` calls it once; later
    /// changes go through [`AppState::set_theme`] and [`AppState::set_system_dark`].
    pub fn apply_theme(&self) {
        let dark = self.is_dark();
        set_visuals(theme::visuals(dark));
        self.theme_state().applied_dark.set(Some(dark));
        Self::changed(&self.theme_state().version);
    }

    fn apply_theme_if_changed(&self) {
        if self.theme_state().applied_dark.get() != Some(self.is_dark()) {
            self.apply_theme();
        }
    }
}

#[cfg(test)]
mod tests {
    use agg_gui::current_visuals;

    use super::*;

    #[test]
    fn toggle_flips_the_installed_look() {
        let state = AppState::new();
        state.set_theme(ThemeChoice::Light);
        state.apply_theme();
        assert!(!current_visuals().is_dark());
        let v = state.theme_version();
        state.toggle_theme();
        assert_eq!(state.theme(), ThemeChoice::Dark);
        assert!(current_visuals().is_dark());
        assert_ne!(state.theme_version(), v);
        state.toggle_theme();
        assert_eq!(state.theme(), ThemeChoice::Light);
        assert!(!current_visuals().is_dark());
    }

    #[test]
    fn match_system_follows_the_reported_appearance() {
        let state = AppState::new();
        state.apply_theme();
        assert!(!state.is_dark(), "unknown OS appearance is light");
        let v = state.theme_version();
        state.set_system_dark(Some(true));
        assert!(state.is_dark());
        assert!(current_visuals().is_dark());
        assert_ne!(state.theme_version(), v);
        let v = state.theme_version();
        state.set_system_dark(Some(true));
        assert_eq!(state.theme_version(), v, "no change, no reinstall");
        // A pinned choice ignores the OS.
        state.set_theme(ThemeChoice::Light);
        state.set_system_dark(Some(false));
        state.set_system_dark(Some(true));
        assert!(!state.is_dark());
    }

    #[test]
    fn returning_to_match_system_reads_the_os_appearance_through_the_probe() {
        let state = AppState::new();
        state.set_system_dark(Some(false));
        state.set_theme(ThemeChoice::Light);
        state.apply_theme();
        // The OS turns dark while Light is pinned; the shell cannot see it, so the last
        // report (light) is stale. The probe reads the real appearance.
        let os_dark = Rc::new(Cell::new(true));
        let calls = Rc::new(Cell::new(0));
        let (probe_dark, probe_calls) = (Rc::clone(&os_dark), Rc::clone(&calls));
        state.set_system_appearance_probe(move || {
            probe_calls.set(probe_calls.get() + 1);
            Some(probe_dark.get())
        });
        state.set_theme(ThemeChoice::Dark);
        assert_eq!(calls.get(), 0, "pinning a look does not probe");
        let v = state.theme_version();
        state.set_theme(ThemeChoice::System);
        assert_eq!(calls.get(), 1);
        assert_eq!(state.system_dark(), Some(true));
        assert!(state.is_dark());
        assert!(current_visuals().is_dark());
        assert_eq!(
            state.theme_version(),
            v,
            "dark was already shown: no reinstall"
        );

        // Back from a pinned Light: dark is installed at once, in one step.
        state.set_theme(ThemeChoice::Light);
        assert!(!current_visuals().is_dark());
        let v = state.theme_version();
        state.set_theme(ThemeChoice::System);
        assert!(current_visuals().is_dark());
        assert_eq!(
            state.theme_version(),
            v + 1,
            "one install, never the stale look"
        );
    }

    #[test]
    fn without_a_probe_match_system_uses_the_last_report() {
        let state = AppState::new();
        state.set_system_dark(Some(true));
        state.set_theme(ThemeChoice::Light);
        state.set_theme(ThemeChoice::System);
        assert!(state.is_dark());
    }

    #[test]
    fn set_theme_installs_the_look_once() {
        let state = AppState::new();
        state.set_theme(ThemeChoice::Light);
        state.apply_theme();
        let v = state.theme_version();
        state.set_theme(ThemeChoice::Dark);
        assert_eq!(state.theme_version(), v + 1);
    }

    #[test]
    fn toggling_from_match_system_pins_the_opposite_look() {
        let state = AppState::new();
        state.set_system_dark(Some(true));
        state.toggle_theme();
        assert_eq!(state.theme(), ThemeChoice::Light);
    }
}
