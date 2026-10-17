//! # taskmanager-core
//!
//! The target-agnostic core of TaskManager, a task manager and system monitor (see
//! `docs/PLAN.md`). Both shells build the same app from here:
//!
//! - `taskmanager-native` opens a window and calls [`build_app`];
//! - `taskmanager-wasm` does the same on a browser canvas.
//!
//! Nothing here is platform-specific (no `cfg(target_os)`); OS code lives in the native
//! shell.
//!
//! Modules:
//! - [`fonts`] — theme + font installation, font sizes and the Font Awesome code points.
//! - [`format`] — human-readable bytes, rates, percentages, temperatures and durations.
//! - [`reading`] — `Reading<T>`: a measured value, or why there is none.
//! - [`ui`] — the widget tree.

pub mod fonts;
pub mod format;
pub mod reading;
pub mod ui;

use agg_gui::App;

pub use fonts::{install_theme_and_fonts, Fonts};
pub use reading::Reading;

/// The product name: window title and the root widget's heading.
pub const APP_TITLE: &str = "TaskManager";

/// Build the whole app. Call [`install_theme_and_fonts`] first.
pub fn build_app(fonts: &Fonts) -> App {
    App::new(ui::build_root(fonts))
}
