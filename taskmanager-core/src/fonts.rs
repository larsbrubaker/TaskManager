//! Theme and fonts, installed once at startup by every shell (and by the headless tests) so
//! native, web and tests render with the same setup, plus the font sizes and the Font
//! Awesome code points the widgets use.
//!
//! The UI face is agg-gui's bundled Noto Sans with its standard fallback chain, which puts
//! Font Awesome 4.7 behind it: an icon is just its code point drawn as ordinary text. The
//! code points the app uses are named in [`fa`]. The look installed here is agg-gui's Light
//! visuals, a neutral starting point: `build_app` then installs the look the user's Light /
//! Dark / Match System choice resolves to (`theme.rs`, `state/theme.rs`).

use std::sync::Arc;

use agg_gui::fonts::{standard_ui_font, FONT_AWESOME_4_7};
use agg_gui::text::Font;
use agg_gui::theme::{set_visuals, Visuals};
use agg_gui::{font_settings, set_device_scale};

/// Point size of a page title ("Summary", "Performance", …) in the title row.
pub const TITLE_FONT_SIZE: f64 = 32.0;

/// Point size of a sidebar row's label (and the footer's Settings label).
pub const SIDEBAR_FONT_SIZE: f64 = 14.0;

/// Point size of a sidebar row's icon.
pub const SIDEBAR_ICON_SIZE: f64 = 14.0;

/// Point size of the sidebar's small-caps group divider ("MORE").
pub const DIVIDER_FONT_SIZE: f64 = 10.5;

/// Point size of the status bar's text.
pub const STATUS_FONT_SIZE: f64 = 13.0;

/// Point size of the word in the sidebar logo.
pub const LOGO_FONT_SIZE: f64 = 21.0;

/// Font Awesome 4.7 code points used by the app (checked against the bundled face in the
/// tests below).
pub mod fa {
    /// fa-tachometer (alias fa-dashboard) — Summary.
    pub const TACHOMETER: char = '\u{f0e4}';
    /// fa-heartbeat — Performance.
    pub const HEARTBEAT: char = '\u{f21e}';
    /// fa-list-alt — Processes.
    pub const LIST_ALT: char = '\u{f022}';
    /// fa-info-circle — System Info, and the notice banner.
    pub const INFO_CIRCLE: char = '\u{f05a}';
    /// fa-power-off — Startup apps.
    pub const POWER_OFF: char = '\u{f011}';
    /// fa-users — Users.
    pub const USERS: char = '\u{f0c0}';
    /// fa-cogs — Services.
    pub const COGS: char = '\u{f085}';
    /// fa-line-chart — Power & Freq.
    pub const LINE_CHART: char = '\u{f201}';
    /// fa-microchip — Processor.
    pub const MICROCHIP: char = '\u{f2db}';
    /// fa-paper-plane — Flight Recorder.
    pub const PAPER_PLANE: char = '\u{f1d8}';
    /// fa-globe — Connections.
    pub const GLOBE: char = '\u{f0ac}';
    /// fa-th-large — Installed Apps.
    pub const TH_LARGE: char = '\u{f009}';
    /// fa-cube — Drivers.
    pub const CUBE: char = '\u{f1b2}';
    /// fa-pie-chart — Disk Space.
    pub const PIE_CHART: char = '\u{f200}';
    /// fa-clock-o — Benchmarks.
    pub const CLOCK_O: char = '\u{f017}';
    /// fa-cog — Settings.
    pub const COG: char = '\u{f013}';
    /// fa-paint-brush — Colors.
    pub const PAINT_BRUSH: char = '\u{f1fc}';
    /// fa-columns — show / hide the sidebar.
    pub const COLUMNS: char = '\u{f0db}';
    /// fa-sun-o — switch to the light theme.
    pub const SUN_O: char = '\u{f185}';
    /// fa-moon-o — switch to the dark theme.
    pub const MOON_O: char = '\u{f186}';
    /// fa-circle — the status bar's status dot.
    pub const CIRCLE: char = '\u{f111}';

    /// Every code point above, for the coverage test.
    pub const ALL: [char; 21] = [
        TACHOMETER,
        HEARTBEAT,
        LIST_ALT,
        INFO_CIRCLE,
        POWER_OFF,
        USERS,
        COGS,
        LINE_CHART,
        MICROCHIP,
        PAPER_PLANE,
        GLOBE,
        TH_LARGE,
        CUBE,
        PIE_CHART,
        CLOCK_O,
        COG,
        PAINT_BRUSH,
        COLUMNS,
        SUN_O,
        MOON_O,
        CIRCLE,
    ];
}

/// The faces the widgets draw with.
#[derive(Clone)]
pub struct Fonts {
    /// Noto Sans with Font Awesome as fallback; used for all text and inline icons.
    pub ui: Arc<Font>,
    /// Font Awesome alone, for `Button::with_icon`.
    pub icons: Arc<Font>,
}

/// Install the Light look and the UI font for a display at `device_scale`, returning the
/// fonts the widget tree is built with.
pub fn install_theme_and_fonts(device_scale: f64) -> Result<Fonts, &'static str> {
    set_device_scale(device_scale);
    set_visuals(Visuals::light());
    let ui = Arc::new(standard_ui_font());
    let icons = Arc::new(Font::from_slice(FONT_AWESOME_4_7)?);
    font_settings::set_system_font(Some(Arc::clone(&ui)));
    Ok(Fonts { ui, icons })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_cover_the_text_and_icons_the_app_draws() {
        let fonts = install_theme_and_fonts(1.0).expect("bundled fonts load");
        for ch in "TaskManager 0123456789.:% °C °F B KB/sec MB GB TB d h min s — ·".chars() {
            assert!(fonts.ui.has_glyph(ch), "UI font is missing {ch:?}");
        }
        for icon in fa::ALL {
            assert!(
                fonts.icons.has_glyph(icon),
                "Font Awesome is missing U+{:04X}",
                icon as u32
            );
            assert!(fonts.ui.has_glyph(icon), "UI fallback chain misses an icon");
        }
    }

    #[test]
    fn icon_code_points_are_distinct() {
        for (i, a) in fa::ALL.iter().enumerate() {
            for b in &fa::ALL[i + 1..] {
                assert_ne!(a, b, "duplicate icon U+{:04X}", *a as u32);
            }
        }
    }
}
