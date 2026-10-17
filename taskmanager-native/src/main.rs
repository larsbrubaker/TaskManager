//! # Native shell for TaskManager
//!
//! Thinnest possible desktop shim: the winit window, event loop, wgpu surface, input
//! forwarding and frame painting all belong to `agg-gui-shell`. This crate names and sizes
//! the window (default 1242 x 791, the reference window; minimum 1100 x 700, PLAN §2), reads
//! the start-up page and theme ([`startup`]), keeps the window's title bar and the OS
//! appearance in step with the theme ([`theme`]) and hands the window the app built by
//! `taskmanager-core`. The native platform (real OS telemetry, [`platform`]) is built but not
//! yet wired to the window (M1.4); remembered window bounds arrive later.
//!
//! Command line ([`cli`]): `--dump-snapshot [N]` samples the machine headless and prints a
//! summary ([`dump`]); `--help` lists the options.
//!
//! `TASKMANAGER_PAGE=<id>` opens a page and `TASKMANAGER_THEME=light|dark|system` picks the
//! look ([`startup`]). `TASKMANAGER_SCREENSHOT=out.png` captures one frame after start-up
//! and exits (checking the UI without a person at the keyboard; Light unless
//! `TASKMANAGER_THEME` says otherwise); `TASKMANAGER_SCREENSHOT_FRAMES=N` sets how many
//! frames to let settle first (default 10).

mod cli;
mod dump;
mod platform;
mod startup;
mod theme;

use std::process::ExitCode;

use agg_gui::App;
use agg_gui_shell::{RedrawPolicy, ShellConfig, ShellControl, ShellError, ShellHost};
use taskmanager_core::{build_app, install_theme_and_fonts, AppState, APP_TITLE};

use startup::Startup;

/// Default window size in logical points: the reference window.
const DEFAULT_SIZE: (f64, f64) = (1242.0, 791.0);

/// Smallest window the layout is designed for, in logical points.
const MIN_SIZE: (f64, f64) = (1100.0, 700.0);

/// Frames to let settle before a `TASKMANAGER_SCREENSHOT` capture, unless overridden.
const DEFAULT_SCREENSHOT_FRAMES: u32 = 10;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&args) {
        Ok(cli::Command::App) => {}
        Ok(cli::Command::DumpSnapshot { samples }) => {
            dump::run(samples);
            return ExitCode::SUCCESS;
        }
        Ok(cli::Command::Help) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("taskmanager: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    }
    let startup = match Startup::from_env() {
        Ok(startup) => startup,
        Err(message) => {
            eprintln!("taskmanager: {message}");
            return ExitCode::from(2);
        }
    };
    match run_app(startup) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("taskmanager: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Open the window and run until it closes.
fn run_app(startup: Startup) -> Result<(), ShellError> {
    let mut config = ShellConfig::new(APP_TITLE)
        .with_logical_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .with_min_logical_size(MIN_SIZE.0, MIN_SIZE.1)
        // Nothing animates yet; repaint only when input or state asks.
        .with_redraw_policy(RedrawPolicy::Reactive)
        .with_device_label("taskmanager-native");
    if let Ok(path) = std::env::var("TASKMANAGER_SCREENSHOT") {
        let frames = std::env::var("TASKMANAGER_SCREENSHOT_FRAMES")
            .ok()
            .and_then(|f| f.parse().ok())
            .unwrap_or(DEFAULT_SCREENSHOT_FRAMES);
        config = config.with_screenshot(path, frames);
    }

    agg_gui_shell::run(config, move |init| {
        let fonts = install_theme_and_fonts(init.device_scale()).map_err(ShellError::app)?;
        let state = AppState::new();
        state.set_page(startup.page);
        state.set_theme(startup.theme);
        // The window's title bar follows the theme; Match System reads the OS appearance,
        // before the first frame so a dark desktop never flashes light.
        let mut window_theme =
            theme::WindowTheme::new(std::sync::Arc::clone(init.window()), &state);
        window_theme.sync(&state);
        let app = build_app(&state, &fonts);
        Ok((
            app,
            AppHost {
                state,
                window_theme,
            },
        ))
    })
}

/// The shell side of the app: keeps the window's appearance and the OS appearance in step
/// with the theme ([`theme`]).
struct AppHost {
    state: AppState,
    window_theme: theme::WindowTheme,
}

impl ShellHost for AppHost {
    fn on_idle(&mut self, _app: &mut App, _control: &mut ShellControl<'_>) {
        self.window_theme.sync(&self.state);
    }
}
