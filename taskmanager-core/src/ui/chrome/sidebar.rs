//! The sidebar (PLAN §2): one row per page (Font Awesome icon + label) with a rounded
//! selection pill behind the current page, the small-caps "MORE" divider between the main
//! group and the rest, our [`logo`](super::logo) near the bottom, and a footer with the
//! Settings button and the sun / moon theme button, split by a thin vertical line.
//!
//! The panel is built with theme colours, so `ui/mod.rs` rebuilds it when the look changes;
//! the rows rebuild on `AppState::page_version` so the pill follows the page (and the
//! Processor row its chip name). Each row is an
//! agg-gui `Clickable` around an icon cell and a label, so every icon sits centred in the
//! same column and every label starts at the same x whatever the glyph's width.

use std::sync::Arc;

use agg_gui::{
    current_visuals, Button, Clickable, Color, Container, FlexColumn, FlexRow, HAnchor, Insets,
    Label, LabelAlign, Rebuilder, Separator, SizedBox, Spacer, VAnchor, Widget,
};

use agg_gui::widgets::ButtonTheme;

use super::logo::Logo;
use super::{
    nav_row_id, selection_fill, BAR_HEIGHT, LINE, LOGO_ID, SETTINGS_BUTTON_ID, SIDEBAR_ID,
    SIDEBAR_WIDTH, THEME_BUTTON_ID,
};
use crate::fonts::{fa, Fonts, DIVIDER_FONT_SIZE, SIDEBAR_FONT_SIZE, SIDEBAR_ICON_SIZE};
use crate::page::{Page, PageGroup, MORE_LABEL};
use crate::state::AppState;

/// Space above the first row.
const TOP_PADDING: f64 = 10.0;
/// Height of a row's selection pill.
const ROW_HEIGHT: f64 = 34.0;
/// Space between two rows' pills.
const ROW_GAP: f64 = 2.5;
/// Gap between the pill and the sidebar's left edge.
const PILL_LEFT: f64 = 8.0;
/// Gap between the pill and the sidebar's separator line.
const PILL_RIGHT: f64 = 10.0;
/// Corner radius of the pill.
const PILL_RADIUS: f64 = 7.0;
/// Inset of the icon cell inside the pill.
const ICON_INSET: f64 = 6.5;
/// Width of the icon cell; icons are centred in it.
const ICON_CELL: f64 = 28.0;
/// Gap between the icon cell and the label.
const LABEL_GAP: f64 = 7.5;
/// Height of the MORE divider row.
const DIVIDER_HEIGHT: f64 = 22.0;
/// Where the divider's lines start and end, from the sidebar's edges.
const DIVIDER_LEFT: f64 = 15.0;
const DIVIDER_RIGHT: f64 = 24.0;
/// Length of the line before the divider's label.
const DIVIDER_LEAD: f64 = 22.0;
/// Space between the divider's label and its lines.
const DIVIDER_LABEL_PAD: f64 = 6.0;
/// Space above and below the logo.
const LOGO_PADDING: f64 = 22.0;
/// Width of the footer's Settings cell (left of the vertical line).
const FOOTER_SETTINGS_WIDTH: f64 = 117.0;
/// Inset of the footer's buttons from the cells' edges.
const FOOTER_BUTTON_INSET: f64 = 4.0;
/// Gap between the footer's vertical line and the bar's top and bottom.
const FOOTER_LINE_INSET: f64 = 7.0;

/// Build the sidebar for the current look.
pub fn build(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let v = current_visuals();
    let (key, rows_state, rows_fonts) = (state.clone(), state.clone(), fonts.clone());
    let rows = Rebuilder::new(
        move || key.page_version(),
        move || nav_rows(&rows_state, &rows_fonts),
    )
    .with_h_anchor(HAnchor::STRETCH);
    let column = FlexColumn::new()
        .with_gap(0.0)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(Box::new(rows))
        .add_flex(Box::new(Spacer::new()), 1.0)
        .add(Box::new(
            Separator::horizontal()
                .with_line_inset(0.0)
                .with_h_anchor(HAnchor::STRETCH),
        ))
        .add(Box::new(
            Logo::new(Arc::clone(&fonts.ui))
                .with_margin(Insets::from_sides(0.0, 0.0, LOGO_PADDING, LOGO_PADDING))
                .with_name(LOGO_ID),
        ))
        .add(footer(state, fonts));
    let panel = Container::new()
        .with_background(v.panel_fill)
        .with_border_sides(v.separator, Insets::from_sides(0.0, LINE, 0.0, 0.0))
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(Box::new(column));
    Box::new(
        SizedBox::new()
            .with_width(SIDEBAR_WIDTH)
            .with_child(Box::new(panel))
            .with_v_anchor(VAnchor::STRETCH)
            .with_name(SIDEBAR_ID),
    )
}

/// The page rows with the MORE divider, for the current page.
fn nav_rows(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let mut column = FlexColumn::new()
        .with_gap(0.0)
        .with_inner_padding(Insets::from_sides(0.0, 0.0, TOP_PADDING, 0.0))
        .with_h_anchor(HAnchor::STRETCH);
    let mut group = PageGroup::Main;
    for page in Page::SIDEBAR {
        if page.group() != group {
            group = page.group();
            column.push(more_divider(fonts), 0.0);
        }
        column.push(nav_row(state, fonts, page), 0.0);
    }
    Box::new(column)
}

/// One page's row: icon cell and label, on the selection pill when it is the current page.
fn nav_row(state: &AppState, fonts: &Fonts, page: Page) -> Box<dyn Widget> {
    let v = current_visuals();
    let selected = state.page() == page;
    let icon = Label::new(page.icon().to_string(), Arc::clone(&fonts.icons))
        .with_font_size(SIDEBAR_ICON_SIZE)
        .with_align(LabelAlign::Center)
        .with_h_anchor(HAnchor::CENTER)
        .with_v_anchor(VAnchor::CENTER);
    let label = Label::new(
        page.sidebar_label(state.processor_brand()),
        Arc::clone(&fonts.ui),
    )
    .with_font_size(SIDEBAR_FONT_SIZE)
    .with_v_anchor(VAnchor::CENTER)
    .with_margin(Insets::from_sides(LABEL_GAP, 0.0, 0.0, 0.0));
    let content = FlexRow::new()
        .with_gap(0.0)
        .with_inner_padding(Insets::from_sides(ICON_INSET, 0.0, 0.0, 0.0))
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(Box::new(
            SizedBox::new()
                .with_width(ICON_CELL)
                .with_height(ROW_HEIGHT)
                .with_child(Box::new(icon)),
        ))
        .add(Box::new(label));
    let pill = if selected {
        selection_fill(&v)
    } else {
        Color::rgba(0.0, 0.0, 0.0, 0.0)
    };
    let click_state = state.clone();
    Box::new(
        Clickable::new(Box::new(
            SizedBox::new()
                .with_height(ROW_HEIGHT)
                .with_child(Box::new(content)),
        ))
        .with_background(pill)
        .with_corner_radius(PILL_RADIUS)
        .with_focus_on_click(false)
        .on_click(move || click_state.set_page(page))
        .with_h_anchor(HAnchor::STRETCH)
        .with_margin(Insets::from_sides(
            PILL_LEFT,
            PILL_RIGHT,
            ROW_GAP / 2.0,
            ROW_GAP / 2.0,
        ))
        .with_name(nav_row_id(page.id())),
    )
}

/// The group divider: a short line, "MORE" in small accent capitals, a long line.
fn more_divider(fonts: &Fonts) -> Box<dyn Widget> {
    let v = current_visuals();
    let line_color = v.accent.with_alpha(0.75);
    let line = || {
        Separator::horizontal()
            .with_line_inset(0.0)
            .with_color(line_color)
            .with_v_anchor(VAnchor::CENTER)
    };
    let label = Label::new(MORE_LABEL, Arc::clone(&fonts.ui))
        .with_font_size(DIVIDER_FONT_SIZE)
        .with_strong(true)
        .with_color(v.accent)
        .with_v_anchor(VAnchor::CENTER)
        .with_margin(Insets::from_sides(
            DIVIDER_LABEL_PAD,
            DIVIDER_LABEL_PAD,
            0.0,
            0.0,
        ));
    Box::new(
        SizedBox::new()
            .with_height(DIVIDER_HEIGHT)
            .with_child(Box::new(
                FlexRow::new()
                    .with_gap(0.0)
                    .with_inner_padding(Insets::from_sides(DIVIDER_LEFT, DIVIDER_RIGHT, 0.0, 0.0))
                    .with_h_anchor(HAnchor::STRETCH)
                    .with_v_anchor(VAnchor::STRETCH)
                    .add(Box::new(
                        SizedBox::new()
                            .with_width(DIVIDER_LEAD)
                            .with_height(DIVIDER_HEIGHT)
                            .with_child(Box::new(line().with_h_anchor(HAnchor::STRETCH))),
                    ))
                    .add(Box::new(label))
                    .add_flex(Box::new(line().with_h_anchor(HAnchor::STRETCH)), 1.0),
            )),
    )
}

/// The footer: Settings | theme button, on the bar colour, under a separator line.
fn footer(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let v = current_visuals();
    let settings_state = state.clone();
    let active_state = state.clone();
    let settings = Button::new(
        Page::Settings.sidebar_label(state.processor_brand()),
        Arc::clone(&fonts.ui),
    )
    .with_font_size(SIDEBAR_FONT_SIZE)
    .with_icon_sized(fa::COG, Arc::clone(&fonts.icons), SIDEBAR_ICON_SIZE)
    .with_ghost()
    .with_theme(pill_theme())
    .with_label_align(LabelAlign::Left)
    .with_active_fn(move || active_state.page() == Page::Settings)
    .with_focus_on_click(false)
    .on_click(move || settings_state.set_page(Page::Settings))
    .with_h_anchor(HAnchor::STRETCH)
    .with_v_anchor(VAnchor::STRETCH)
    .with_margin(Insets::all(FOOTER_BUTTON_INSET))
    .with_name(SETTINGS_BUTTON_ID);
    let row = FlexRow::new()
        .with_gap(0.0)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .add(Box::new(
            SizedBox::new()
                .with_width(FOOTER_SETTINGS_WIDTH)
                .with_height(BAR_HEIGHT)
                .with_child(Box::new(settings)),
        ))
        .add(Box::new(
            Separator::vertical()
                .with_line_inset(0.0)
                .with_v_anchor(VAnchor::STRETCH)
                .with_margin(Insets::from_sides(
                    0.0,
                    0.0,
                    FOOTER_LINE_INSET,
                    FOOTER_LINE_INSET,
                )),
        ))
        .add_flex(theme_button(state, fonts), 1.0);
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
                    .add(Box::new(row)),
            )),
    )
}

/// The sun / moon button: shows the look it switches to and flips Light / Dark.
fn theme_button(state: &AppState, fonts: &Fonts) -> Box<dyn Widget> {
    let (icon, label, tip) = if state.is_dark() {
        (fa::SUN_O, "Light mode", "Switch to the light theme")
    } else {
        (fa::MOON_O, "Dark mode", "Switch to the dark theme")
    };
    let toggle_state = state.clone();
    Box::new(
        Button::new(label, Arc::clone(&fonts.ui))
            .with_font_size(SIDEBAR_FONT_SIZE)
            .with_icon_sized(icon, Arc::clone(&fonts.icons), SIDEBAR_ICON_SIZE)
            .with_ghost()
            .with_label_align(LabelAlign::Left)
            .with_active_fn(|| false)
            .with_focus_on_click(false)
            .on_click(move || toggle_state.toggle_theme())
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH)
            .with_margin(Insets::all(FOOTER_BUTTON_INSET))
            .with_tooltip(tip)
            .with_name(THEME_BUTTON_ID),
    )
}

/// A ghost button's look when active: the selection pill under ordinary text, as the
/// sidebar rows show the current page.
fn pill_theme() -> ButtonTheme {
    let v = current_visuals();
    let fill = selection_fill(&v);
    ButtonTheme {
        background: fill,
        background_hovered: fill,
        background_pressed: fill,
        label_color: v.text_color,
        border_radius: PILL_RADIUS,
        focus_ring_color: v.accent_focus,
        focus_ring_width: 2.5,
    }
}
