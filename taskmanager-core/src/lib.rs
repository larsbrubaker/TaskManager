//! # taskmanager-core
//!
//! The target-agnostic core of TaskManager, a task manager and system monitor (see
//! `docs/PLAN.md`). Both shells build the same app from here:
//!
//! - `taskmanager-native` opens a window, makes an [`AppState`] (start page, theme, OS
//!   appearance) and calls [`build_app`];
//! - `taskmanager-wasm` does the same on a browser canvas.
//!
//! Nothing here is platform-specific (no `cfg(target_os)`); OS code lives in the native
//! shell.
//!
//! Modules:
//! - [`fonts`] — theme + font installation, font sizes and the Font Awesome code points.
//! - [`format`] — human-readable bytes, rates, percentages, temperatures and durations.
//! - [`page`] — the destinations: ids, titles, sidebar labels and icons.
//! - [`reading`] — `Reading<T>`: a measured value, or why there is none.
//! - [`state`] — `AppState`: current page, theme, sidebar, status line.
//! - [`theme`] — Light / Dark / Match System and the channel colours.
//! - [`ui`] — the widget tree: the window chrome and the page host.

pub mod fonts;
pub mod format;
pub mod page;
pub mod reading;
pub mod state;
pub mod theme;
pub mod ui;

use agg_gui::App;

pub use fonts::{install_theme_and_fonts, Fonts};
pub use page::{Page, ProcessorBrand};
pub use reading::Reading;
pub use state::AppState;
pub use theme::ThemeChoice;

/// The product name: window title, logo and status line.
pub const APP_TITLE: &str = "TaskManager";

/// Build the whole app over `state`. Call [`install_theme_and_fonts`] first; this then
/// installs the look `state`'s theme choice resolves to.
pub fn build_app(state: &AppState, fonts: &Fonts) -> App {
    state.apply_theme();
    App::new(ui::build_root(state, fonts))
}
