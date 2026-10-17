//! The title row across the top of the content area (PLAN §2): the show / hide sidebar
//! button, the page title set large in agg-gui's default font, and on the right a slot for
//! the page's actions (filter field, view combo, Reload, Export…). The pages fill the slot
//! as they land (M3 on); until then it is empty. A separator line runs under the row.
//!
//! Built with theme colours; `ui/mod.rs` rebuilds it on `AppState::page_version` (new
//! title) and with the whole row on a look change. The sidebar toggle alone rebuilds on
//! `AppState::sidebar_version`, so its tooltip says which way it goes without rebuilding
//! anything else.

use std::sync::Arc;

use agg_gui::{
    current_visuals, Button, Container, FlexRow, HAnchor, Insets, Label, LabelAlign, Rebuilder,
    SizedBox, Spacer, VAnchor, Widget,
};

use super::{LINE, PAGE_ACTIONS_ID, PAGE_TITLE_ID, SIDEBAR_TOGGLE_ID, TITLE_ROW_HEIGHT};
use crate::fonts::{fa, Fonts, TITLE_FONT_SIZE};
use crate::state::AppState;

/// Gap between the content area's left edge and the sidebar button.
const LEFT_PADDING: f64 = 12.0;
/// Gap between the right edge and the page actions.
const RIGHT_PADDING: f64 = 16.0;
/// Point size of the sidebar button's icon.
const TOGGLE_ICON_SIZE: f64 = 18.0;
/// Width and height of the sidebar button.
const TOGGLE_SIZE: f64 = 34.0;
/// Gap between the sidebar button and the title.
const TITLE_GAP: f64 = 9.0;
/// The sidebar button's tooltip while the sidebar is shown.
pub const SIDEBAR_HIDE_TIP: &str = "Hide the sidebar";
/// The sidebar button's tooltip while the sidebar is hidden.
pub const SIDEBAR_SHOW_TIP: &str = "Show the sidebar";

/// Build the title row for the current page and look.
pub fn build(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let v = current_visuals();
    let (key, toggle_state, toggle_fonts) = (state.clone(), state.clone(), fonts.clone());
    let toggle = Rebuilder::new(
        move || key.sidebar_version(),
        move || sidebar_toggle(&toggle_state, &toggle_fonts),
    )
    .with_h_anchor(HAnchor::STRETCH)
    .with_v_anchor(VAnchor::CENTER);
    let title = Label::new(state.page_title(), Arc::clone(&fonts.ui))
        .with_font_size(TITLE_FONT_SIZE)
        .with_align(LabelAlign::Left)
        .with_v_anchor(VAnchor::CENTER)
        .with_margin(Insets::from_sides(TITLE_GAP, 0.0, 0.0, 0.0))
        .with_name(PAGE_TITLE_ID);
    let actions = FlexRow::new()
        .with_v_anchor(VAnchor::CENTER)
        .with_name(PAGE_ACTIONS_ID);
    let row = FlexRow::new()
        .with_gap(0.0)
        .with_inner_padding(Insets::from_sides(LEFT_PADDING, RIGHT_PADDING, 0.0, 0.0))
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(Box::new(
            SizedBox::new()
                .with_width(TOGGLE_SIZE)
                .with_height(TOGGLE_SIZE)
                .with_child(Box::new(toggle))
                .with_v_anchor(VAnchor::CENTER),
        ))
        .add(Box::new(title))
        .add_flex(Box::new(Spacer::new()), 1.0)
        .add(Box::new(actions));
    Box::new(
        SizedBox::new()
            .with_height(TITLE_ROW_HEIGHT + LINE)
            .with_h_anchor(HAnchor::STRETCH)
            .with_child(Box::new(
                Container::new()
                    .with_background(v.window_fill)
                    .with_border_sides(v.separator, Insets::from_sides(0.0, 0.0, 0.0, LINE))
                    .with_h_anchor(HAnchor::STRETCH)
                    .with_v_anchor(VAnchor::STRETCH)
                    // The container stacks its child at its natural height: give the row
                    // the full height so the button and title centre in it.
                    .add(Box::new(
                        SizedBox::new()
                            .with_height(TITLE_ROW_HEIGHT)
                            .with_h_anchor(HAnchor::STRETCH)
                            .with_child(Box::new(row)),
                    )),
            )),
    )
}

/// The show / hide sidebar button, its tooltip naming what a click does now.
fn sidebar_toggle(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let toggle_state = state.clone();
    let tip = if state.sidebar_visible() {
        SIDEBAR_HIDE_TIP
    } else {
        SIDEBAR_SHOW_TIP
    };
    Box::new(
        Button::new("", Arc::clone(&fonts.ui))
            .with_icon_sized(fa::COLUMNS, Arc::clone(&fonts.icons), TOGGLE_ICON_SIZE)
            .with_ghost()
            .with_active_fn(|| false)
            .with_compact()
            .with_focus_on_click(false)
            .on_click(move || toggle_state.toggle_sidebar())
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::CENTER)
            .with_tooltip(tip)
            .with_name(SIDEBAR_TOGGLE_ID),
    )
}
