//! The widget tree. Built only from agg-gui widgets.
//!
//! [`build_root`] is the app's root widget: for now a full-window panel in the current
//! look's window colour holding the product name as a title. The shared chrome (sidebar,
//! title row, page host, status bar; PLAN §2) replaces it in the next M0 step.

use std::sync::Arc;

use agg_gui::{current_visuals, Container, HAnchor, Insets, Label, VAnchor, Widget};

use crate::fonts::{Fonts, TITLE_FONT_SIZE};
use crate::APP_TITLE;

/// Widget id of the title label.
pub const TITLE_ID: &str = "title";

/// Space around the title, in logical points.
const TITLE_PADDING: f64 = 24.0;

/// The app's root widget. Call [`crate::install_theme_and_fonts`] first: the panel takes
/// its colours from the installed look.
pub fn build_root(fonts: &Fonts) -> Box<dyn Widget> {
    let visuals = current_visuals();
    let title = Label::new(APP_TITLE, Arc::clone(&fonts.ui))
        .with_font_size(TITLE_FONT_SIZE)
        .with_color(visuals.text_color)
        .with_h_anchor(HAnchor::LEFT)
        .with_v_anchor(VAnchor::TOP)
        .with_name(TITLE_ID);
    Box::new(
        Container::new()
            .with_background(visuals.window_fill)
            .with_inner_padding(Insets::all(TITLE_PADDING))
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH)
            .add(Box::new(title)),
    )
}
