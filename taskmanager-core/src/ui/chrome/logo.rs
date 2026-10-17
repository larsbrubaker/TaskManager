//! Our logo, drawn near the bottom of the sidebar: a heartbeat (ECG) trace in the accent
//! colour with a soft glow, followed by the word "TaskManager". It is our own mark (PLAN
//! §1: a monitor's heartbeat line, not a copy of anyone's wordmark), painted with agg-gui
//! vector paths and text so it is crisp at every scale and follows the look.
//!
//! The glow is the trace stroked a few times underneath itself, widest and faintest first,
//! the same stacked-alpha technique agg-gui's window chrome uses for its soft shadow.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    DrawCtx, Event, EventResult, HAnchor, Insets, LineCap, LineJoin, Rect, Size, VAnchor, Widget,
    WidgetBase,
};

use crate::fonts::LOGO_FONT_SIZE;
use crate::APP_TITLE;

/// Height of the logo, its margins excluded.
pub const LOGO_HEIGHT: f64 = 40.0;
/// Inset of the trace from the logo's left edge.
const LEFT: f64 = 20.0;
/// Width of the heartbeat trace.
const TRACE_WIDTH: f64 = 56.0;
/// Half the height the trace's tallest spike reaches above its baseline.
const TRACE_AMPLITUDE: f64 = 17.0;
/// Gap between the trace and the word.
const WORD_GAP: f64 = 7.0;
/// Width of the trace's core line.
const CORE_WIDTH: f64 = 2.0;
/// The glow passes under the core line: (line width, opacity), outermost first.
const GLOW: [(f64, f32); 3] = [(10.0, 0.07), (6.5, 0.12), (4.0, 0.22)];

/// One heartbeat, as (x, y) fractions of the trace box: x from 0 to 1 left to right, y
/// from -1 (lowest dip) to 1 (top of the spike) about the baseline. A flat run, the small
/// P wave, the Q dip, the tall R spike, the S dip, the T wave and a flat run again.
const BEAT: [(f64, f64); 13] = [
    (0.00, 0.00),
    (0.16, 0.00),
    (0.22, 0.14),
    (0.28, 0.00),
    (0.35, 0.00),
    (0.39, -0.20),
    (0.46, 1.00),
    (0.53, -0.78),
    (0.58, 0.00),
    (0.67, 0.00),
    (0.75, 0.24),
    (0.83, 0.00),
    (1.00, 0.00),
];

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

    /// The trace's points in the widget's local Y-up space.
    fn trace_points(&self) -> impl Iterator<Item = (f64, f64)> {
        let mid = LOGO_HEIGHT / 2.0;
        BEAT.iter()
            .map(move |(x, y)| (LEFT + x * TRACE_WIDTH, mid + y * TRACE_AMPLITUDE))
    }

    fn trace_path(&self, ctx: &mut dyn DrawCtx) {
        ctx.begin_path();
        for (i, (x, y)) in self.trace_points().enumerate() {
            if i == 0 {
                ctx.move_to(x, y);
            } else {
                ctx.line_to(x, y);
            }
        }
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
        ctx.set_line_join(LineJoin::Round);
        ctx.set_line_cap(LineCap::Round);
        for (width, alpha) in GLOW {
            ctx.set_stroke_color(v.accent.with_alpha(alpha));
            ctx.set_line_width(width);
            self.trace_path(ctx);
            ctx.stroke();
        }
        ctx.set_stroke_color(v.accent);
        ctx.set_line_width(CORE_WIDTH);
        self.trace_path(ctx);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(LOGO_FONT_SIZE);
        ctx.set_fill_color(v.text_color);
        if let Some(m) = ctx.measure_text(APP_TITLE) {
            let baseline = m.centered_baseline_y(LOGO_HEIGHT);
            ctx.fill_text(APP_TITLE, LEFT + TRACE_WIDTH + WORD_GAP, baseline);
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
    fn the_beat_runs_left_to_right_within_its_box() {
        for pair in BEAT.windows(2) {
            assert!(pair[1].0 > pair[0].0, "x goes backwards at {pair:?}");
        }
        assert!(BEAT.iter().all(|(_, y)| (-1.0..=1.0).contains(y)));
        assert_eq!(BEAT.iter().map(|(_, y)| *y).fold(f64::MIN, f64::max), 1.0);
    }
}
