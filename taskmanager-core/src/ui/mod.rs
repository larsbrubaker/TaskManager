//! The widget tree. Built only from agg-gui widgets (plus our own `chrome::logo`).
//!
//! [`build_root`] is the app's root: a `Stack` (overlays such as toasts and sheets join it
//! as they land) holding the window's one row: the [`chrome::sidebar`] on the left, shown
//! or hidden by a `Conditional` on `AppState`'s sidebar flag, and the content column: the
//! [`chrome::title_row`], the page host and the [`chrome::status_bar`] (PLAN §2).
//!
//! Everything the chrome builds with a theme colour rebuilds when the look changes (the row
//! is a `Rebuilder` on `AppState::theme_version`). Inside it each part rebuilds only on
//! the change it shows: the title row and page host on `AppState::page_version` (a page
//! change shows its title and content), the status bar on `AppState::status_version`.
//! Showing or hiding the sidebar rebuilds neither: the `Conditional` reads the flag and
//! only the title row's toggle button rebuilds (on `AppState::sidebar_version`).
//!
//! The page host shows the current page's content area. The pages arrive from M3 on; until
//! then every page's area is empty, the declared M0 state (PLAN §9).

pub mod chrome;

use agg_gui::{
    current_visuals, Conditional, Container, FlexColumn, FlexRow, HAnchor, Rebuilder, Stack,
    VAnchor, Widget,
};

use crate::fonts::Fonts;
use crate::state::AppState;
use chrome::page_content_id;

/// The app's root widget. `build_app` installs the look first.
pub fn build_root(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let (key, row_state, row_fonts) = (state.clone(), state.clone(), fonts.clone());
    let row = Rebuilder::new(
        move || key.theme_version(),
        move || window_row(&row_state, &row_fonts),
    )
    .with_h_anchor(HAnchor::STRETCH)
    .with_v_anchor(VAnchor::STRETCH);
    Box::new(
        Stack::new()
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH)
            .add(Box::new(row)),
    )
}

/// Sidebar | (title row / page host / status bar), for the current look.
fn window_row(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let sidebar = Conditional::new(
        state.sidebar_visibility(),
        chrome::sidebar::build(state, fonts),
    )
    .with_v_anchor(VAnchor::STRETCH);
    let content = FlexColumn::new()
        .with_gap(0.0)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(versioned(
            state,
            fonts,
            AppState::page_version,
            chrome::title_row::build,
        ))
        .add_flex(
            versioned(state, fonts, AppState::page_version, page_host),
            1.0,
        )
        .add(versioned(
            state,
            fonts,
            AppState::status_version,
            chrome::status_bar::build,
        ));
    Box::new(
        FlexRow::new()
            .with_gap(0.0)
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH)
            .add(Box::new(sidebar))
            .add_flex(Box::new(content), 1.0),
    )
}

/// `build`, rebuilt whenever the counter `version` reads moves.
fn versioned(
    state: &AppState,
    fonts: &Fonts,
    version: fn(&AppState) -> u64,
    build: fn(&AppState, &Fonts) -> Box<dyn Widget>,
) -> Box<dyn Widget> {
    let (key, state, fonts) = (state.clone(), state.clone(), fonts.clone());
    Box::new(
        // No vertical stretch: in the column that would make the bars grow too (the page
        // host is the column's one flex child).
        Rebuilder::new(move || version(&key), move || build(&state, &fonts))
            .with_h_anchor(HAnchor::STRETCH),
    )
}

/// The current page's content area: empty until the pages land (M3 on).
fn page_host(state: &AppState, _fonts: &Fonts) -> Box<dyn Widget> {
    let v = current_visuals();
    Box::new(
        Container::new()
            .with_background(v.bg_color)
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH)
            .with_name(page_content_id(state.page().id())),
    )
}
