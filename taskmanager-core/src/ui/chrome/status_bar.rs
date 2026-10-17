//! The status bar along the bottom of the content area (PLAN §2), under a separator line
//! and level with the sidebar footer. Its left side is a status dot and a line of text from
//! `AppState::status` (for now "TaskManager <version>" on a neutral dot; the telemetry
//! health, process count, IP and sample generation arrive with M1). The right side is left
//! free: the Flight Recorder transport sits there from M11.
//!
//! Built with theme colours; `ui/mod.rs` rebuilds it on `AppState::status_version` (and
//! with the whole row on a look change).

use std::sync::Arc;

use agg_gui::theme::Visuals;
use agg_gui::{
    current_visuals, Color, Container, FlexRow, HAnchor, Insets, Label, SizedBox, VAnchor, Widget,
};

use super::{BAR_HEIGHT, LINE, STATUS_DOT_ID, STATUS_TEXT_ID};
use crate::fonts::{fa, Fonts, STATUS_FONT_SIZE};
use crate::state::{AppState, StatusLevel};

/// Gap between the content area's left edge and the dot.
const LEFT_PADDING: f64 = 14.0;
/// Point size of the dot (a Font Awesome circle).
const DOT_SIZE: f64 = 9.0;
/// Gap between the dot and the text.
const TEXT_GAP: f64 = 8.0;

/// The dot's colour for a status level.
pub fn dot_color(level: StatusLevel, v: &Visuals) -> Color {
    match level {
        StatusLevel::Neutral => v.text_dim,
        StatusLevel::Healthy => v.success_color(),
        StatusLevel::Degraded => v.warning_color(),
    }
}

/// Build the status bar for the current status and look.
pub fn build(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let v = current_visuals();
    let status = state.status();
    let dot = Label::new(fa::CIRCLE.to_string(), Arc::clone(&fonts.icons))
        .with_font_size(DOT_SIZE)
        .with_color(dot_color(status.level, &v))
        .with_v_anchor(VAnchor::CENTER)
        .with_name(STATUS_DOT_ID);
    let text = Label::new(status.text, Arc::clone(&fonts.ui))
        .with_font_size(STATUS_FONT_SIZE)
        .with_color(v.text_dim)
        .with_v_anchor(VAnchor::CENTER)
        .with_margin(Insets::from_sides(TEXT_GAP, 0.0, 0.0, 0.0))
        .with_name(STATUS_TEXT_ID);
    let row = FlexRow::new()
        .with_gap(0.0)
        .with_inner_padding(Insets::from_sides(LEFT_PADDING, 0.0, 0.0, 0.0))
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(Box::new(dot))
        .add(Box::new(text));
    Box::new(
        SizedBox::new()
            .with_height(BAR_HEIGHT + LINE)
            .with_h_anchor(HAnchor::STRETCH)
            .with_child(Box::new(
                Container::new()
                    .with_background(v.window_fill)
                    .with_border_sides(v.separator, Insets::from_sides(0.0, 0.0, LINE, 0.0))
                    .with_h_anchor(HAnchor::STRETCH)
                    .with_v_anchor(VAnchor::STRETCH)
                    // The container stacks its child at its natural height: give the row
                    // the bar's full height so the dot and text centre in it.
                    .add(Box::new(
                        SizedBox::new()
                            .with_height(BAR_HEIGHT)
                            .with_h_anchor(HAnchor::STRETCH)
                            .with_child(Box::new(row)),
                    )),
            )),
    )
}
