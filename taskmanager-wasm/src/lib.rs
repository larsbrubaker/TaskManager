//! # WebAssembly shell for the TaskManager demo
//!
//! Thinnest possible browser shim: the canvas, the WebGPU / WebGL2 surface, the
//! requestAnimationFrame loop, DOM input and DPR tracking all belong to
//! `agg-gui-web-shell`. This crate installs the theme and fonts and boots the shared app
//! from `taskmanager-core` on the canvas with id [`CANVAS_ID`]. Match System follows the
//! browser's `prefers-color-scheme` (watched with `matchMedia`, so it switches live). The
//! simulated demo machine (PLAN §4.3) and the GitHub Pages page arrive with later
//! milestones.
//!
//! On native targets the crate is empty (it is a workspace member only so fmt and clippy
//! cover it).

#![cfg(target_arch = "wasm32")]

use agg_gui_web_shell::{start, NoHost, RedrawPolicy, WebShellConfig, WebShellError};
use taskmanager_core::{build_app, install_theme_and_fonts, AppState, APP_TITLE};
use wasm_bindgen::prelude::*;

/// `id` of the canvas the page provides.
pub const CANVAS_ID: &str = "taskmanager-canvas";

#[wasm_bindgen(inline_js = "function taskmanager_scheme() { \
    try { return window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null; } \
    catch (e) { return null; } } \
    export function taskmanager_prefers_dark() { \
    const q = taskmanager_scheme(); return q ? q.matches : undefined; } \
    export function taskmanager_watch_color_scheme(cb) { \
    const q = taskmanager_scheme(); if (!q || !q.addEventListener) { return false; } \
    q.addEventListener('change', (e) => cb(e.matches)); return true; }")]
extern "C" {
    /// The page's `prefers-color-scheme: dark` (`None` without `matchMedia`).
    fn taskmanager_prefers_dark() -> Option<bool>;
    /// Call `cb(dark)` whenever `prefers-color-scheme` changes; false when it cannot.
    fn taskmanager_watch_color_scheme(cb: &Closure<dyn Fn(bool)>) -> bool;
}

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    let config = WebShellConfig::new(CANVAS_ID)
        .with_app_name(APP_TITLE)
        .with_device_label("taskmanager-wasm")
        .with_redraw_policy(RedrawPolicy::Reactive);
    // Errors are already on the console and in the shell's fatal panel; returning them
    // also fails the wasm start so the page's boot code sees it.
    start(config, |init| {
        let fonts = install_theme_and_fonts(init.device_scale()).map_err(WebShellError::app)?;
        let state = AppState::new();
        // Match System: the browser's colour scheme, now and as it changes.
        state.set_system_dark(taskmanager_prefers_dark());
        let watched = state.clone();
        let on_change = Closure::<dyn Fn(bool)>::new(move |dark| {
            watched.set_system_dark(Some(dark));
        });
        if taskmanager_watch_color_scheme(&on_change) {
            // Lives as long as the page.
            on_change.forget();
        }
        Ok((build_app(&state, &fonts), NoHost))
    })
    .map_err(|err| JsValue::from_str(&err.to_string()))
}
