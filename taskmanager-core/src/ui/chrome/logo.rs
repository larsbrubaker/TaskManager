//! Our logo, drawn near the bottom of the sidebar: three short LED meter columns (CPU
//! green, memory magenta, GPU blue, from the theme's `ChannelPalette`) followed by the word
//! "TaskManager". It borrows the look of our own instruments (PLAN §3 SegmentBar), so it
//! is ours and deliberately unlike TMOG's mark: flat cells, no glow, no heartbeat line.
//! Painted with agg-gui vector paths and text so it is crisp at every scale and follows
//! the look.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    DrawCtx, Event, EventResult, HAnchor, Insets, Rect, Size, VAnchor, Widget, WidgetBase,
};

use crate::fonts::LOGO_FONT_SIZE;
use crate::theme::{Channel, ChannelPalette};
use crate::APP_TITLE;

/// Height of the logo, its margins excluded.
pub const LOGO_HEIGHT: f64 = 40.0;
/// Inset of the first column from the logo's left edge.
const LEFT: f64 = 20.0;
/// Cells per column.
const CELLS: usize = 5;
/// Size of one LED cell.
const CELL_WIDTH: f64 = 10.0;
const CELL_HEIGHT: f64 = 4.0;
/// Corner radius of a cell.
const CELL_RADIUS: f64 = 1.5;
/// Vertical gap between cells and horizontal gap between columns.
const CELL_GAP: f64 = 2.0;
const COLUMN_GAP: f64 = 3.0;
/// Gap between the last column and the word.
const WORD_GAP: f64 = 10.0;
/// Opacity of an unlit cell, drawn in the text colour.
const UNLIT_ALPHA: f32 = 0.14;

/// The columns, left to right: which channel colours them and how many cells are lit.
const COLUMNS: [(Channel, usize); 3] = [(Channel::Cpu, 3), (Channel::Memory, 5), (Channel::Gpu, 2)];

/// Width of the meter block (all columns and the gaps between them).
fn meters_width() -> f64 {
    let n = COLUMNS.len() as f64;
    n * CELL_WIDTH + (n - 1.0) * COLUMN_GAP
}

/// Height of one column.
fn column_height() -> f64 {
    let n = CELLS as f64;
    n * CELL_HEIGHT + (n - 1.0) * CELL_GAP
}

/// See the module docs.
pub struct Logo {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    font: Arc<Font>,
}

impl Logo {
    /// A logo whose word is set in `font`.
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new().with_h_anchor(HAnchor::STRETCH),
            font,
        }
    }

    /// Space kept clear around the logo by its parent's layout (the sidebar gives it
    /// room above and below).
    pub fn with_margin(mut self, margin: Insets) -> Self {
        self.base.margin = margin;
        self
    }
}

impl Widget for Logo {
    fn type_name(&self) -> &'static str {
        "Logo"
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, LOGO_HEIGHT)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let palette = ChannelPalette::for_look(v.is_dark());
        let unlit = v.text_color.with_alpha(UNLIT_ALPHA);
        // Local space is Y-up: cell 0 is the bottom one, and lit cells fill from the bottom.
        let bottom = (LOGO_HEIGHT - column_height()) / 2.0;
        for (col, (channel, lit)) in COLUMNS.iter().enumerate() {
            let x = LEFT + col as f64 * (CELL_WIDTH + COLUMN_GAP);
            for cell in 0..CELLS {
                let y = bottom + cell as f64 * (CELL_HEIGHT + CELL_GAP);
                let color = if cell < *lit {
                    palette.color(*channel)
                } else {
                    unlit
                };
                ctx.set_fill_color(color);
                ctx.begin_path();
                ctx.rounded_rect(x, y, CELL_WIDTH, CELL_HEIGHT, CELL_RADIUS);
                ctx.fill();
            }
        }

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(LOGO_FONT_SIZE);
        ctx.set_fill_color(v.text_color);
        if let Some(m) = ctx.measure_text(APP_TITLE) {
            let baseline = m.centered_baseline_y(LOGO_HEIGHT);
            ctx.fill_text(APP_TITLE, LEFT + meters_width() + WORD_GAP, baseline);
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meters_fit_inside_the_logo() {
        assert!(column_height() < LOGO_HEIGHT);
        for (_, lit) in COLUMNS {
            assert!(lit <= CELLS, "a column lights more cells than it has");
        }
    }

    #[test]
    fn the_columns_use_distinct_channels() {
        for (i, (a, _)) in COLUMNS.iter().enumerate() {
            for (b, _) in &COLUMNS[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
