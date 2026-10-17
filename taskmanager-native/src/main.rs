//! # Native shell for TaskManager
//!
//! Thinnest possible desktop shim: the winit window, event loop, wgpu surface, input
//! forwarding and frame painting all belong to `agg-gui-shell`. This crate names and sizes
//! the window (default 1242 x 791, the reference window; minimum 1100 x 700, PLAN §2) and
//! hands it the app built by `taskmanager-core`. The real OS telemetry provider, command
//! line and remembered window bounds arrive with later milestones.
//!
//! `TASKMANAGER_SCREENSHOT=out.png` captures one frame after start-up and exits (checking
//! the UI without a person at the keyboard); `TASKMANAGER_SCREENSHOT_FRAMES=N` sets how many
//! frames to let settle first (default 10).

use std::process::ExitCode;

use agg_gui_shell::{NoHost, RedrawPolicy, ShellConfig, ShellError};
use taskmanager_core::{build_app, install_theme_and_fonts, APP_TITLE};

/// Default window size in logical points: the reference window.
const DEFAULT_SIZE: (f64, f64) = (1242.0, 791.0);

/// Smallest window the layout is designed for, in logical points.
const MIN_SIZE: (f64, f64) = (1100.0, 700.0);

/// Frames to let settle before a `TASKMANAGER_SCREENSHOT` capture, unless overridden.
const DEFAULT_SCREENSHOT_FRAMES: u32 = 10;

fn main() -> ExitCode {
    match run_app() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("taskmanager: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Open the window and run until it closes.
fn run_app() -> Result<(), ShellError> {
    let mut config = ShellConfig::new(APP_TITLE)
        .with_logical_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .with_min_logical_size(MIN_SIZE.0, MIN_SIZE.1)
        // Nothing animates yet; repaint only when input asks.
        .with_redraw_policy(RedrawPolicy::Reactive)
        .with_device_label("taskmanager-native");
    if let Ok(path) = std::env::var("TASKMANAGER_SCREENSHOT") {
        let frames = std::env::var("TASKMANAGER_SCREENSHOT_FRAMES")
            .ok()
            .and_then(|f| f.parse().ok())
            .unwrap_or(DEFAULT_SCREENSHOT_FRAMES);
        config = config.with_screenshot(path, frames);
    }

    agg_gui_shell::run(config, |init| {
        let fonts = install_theme_and_fonts(init.device_scale()).map_err(ShellError::app)?;
        Ok((build_app(&fonts), NoHost))
    })
}
