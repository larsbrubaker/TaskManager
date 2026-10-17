//! The shared window chrome (PLAN §2): the [`sidebar`] (page rows, the MORE divider, the
//! [`logo`], the Settings / theme footer), the [`title_row`] (sidebar toggle, page title,
//! page actions) and the [`status_bar`]. `ui/mod.rs` arranges them around the page host.
//!
//! The metrics below are the reference window's (`reference/app/*.png`, captured at 2x),
//! in logical points. The footer and the status bar share [`BAR_HEIGHT`] so they read as
//! one strip across the bottom of the window.

pub mod logo;
pub mod sidebar;
pub mod status_bar;
pub mod title_row;

use agg_gui::theme::Visuals;
use agg_gui::Color;

/// Width of the sidebar, its right-hand separator line included.
pub const SIDEBAR_WIDTH: f64 = 240.0;

/// Height of the title row, its bottom separator line excluded.
pub const TITLE_ROW_HEIGHT: f64 = 53.0;

/// Height of the status bar and of the sidebar footer, their top separator line excluded.
pub const BAR_HEIGHT: f64 = 36.0;

/// Width of the separator lines between the chrome's parts.
pub const LINE: f64 = 1.0;

/// Widget id of the sidebar (absent from the tree's visible set while hidden).
pub const SIDEBAR_ID: &str = "sidebar";
/// Widget id of the title row's show / hide sidebar button.
pub const SIDEBAR_TOGGLE_ID: &str = "sidebar-toggle";
/// Widget id of the title row's page title label.
pub const PAGE_TITLE_ID: &str = "page-title";
/// Widget id of the title row's page-actions slot.
pub const PAGE_ACTIONS_ID: &str = "page-actions";
/// Widget id of the sidebar footer's Settings button.
pub const SETTINGS_BUTTON_ID: &str = "settings-button";
/// Widget id of the sidebar footer's sun / moon button.
pub const THEME_BUTTON_ID: &str = "theme-toggle";
/// Widget id of the status bar's dot.
pub const STATUS_DOT_ID: &str = "status-dot";
/// Widget id of the status bar's text.
pub const STATUS_TEXT_ID: &str = "status-text";
/// Widget id of the sidebar logo.
pub const LOGO_ID: &str = "logo";

/// Widget id of the sidebar row for the page with id `page_id`.
pub fn nav_row_id(page_id: &str) -> String {
    format!("nav-{page_id}")
}

/// Widget id of the page host's content for the page with id `page_id`.
pub fn page_content_id(page_id: &str) -> String {
    format!("page-{page_id}")
}

/// The selection pill behind the current page's row: a pale wash of the accent over the
/// sidebar panel, so the row's text keeps the ordinary text colour.
pub fn selection_fill(v: &Visuals) -> Color {
    let alpha = if v.is_dark() { 0.38 } else { 0.24 };
    v.accent.with_alpha(alpha)
}
