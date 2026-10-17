//! Shared harness for the headless UI tests: the real production tree (`build_app`) over an
//! `AppState`, with no window and no GPU. Each frame drains agg-gui's `ui_thread` queue,
//! lays out at the reference window size and paints into a software framebuffer; clicks go
//! through the same `App` mouse entry points a shell uses. Widgets are found by their ids.

#![allow(dead_code)]

use agg_gui::widget::{find_widget_by_id, find_widget_by_id_mut, find_widget_screen_rect};
use agg_gui::{App, Framebuffer, GfxCtx, Label, Modifiers, MouseButton, Rect, Size};
use taskmanager_core::{build_app, install_theme_and_fonts, AppState};

/// The reference window, in logical points (the harness runs at scale 1).
pub const WIDTH: f64 = 1242.0;
pub const HEIGHT: f64 = 791.0;

/// Tooltip text [`Harness::mark`] uses to tag a widget instance.
const REBUILD_MARK: &str = "rebuild-mark";

/// The real app over a state, laid out and painted once.
pub struct Harness {
    pub state: AppState,
    pub app: App,
    pub size: Size,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_state(AppState::new())
    }

    pub fn with_state(state: AppState) -> Self {
        let fonts = install_theme_and_fonts(1.0).expect("bundled fonts load");
        let app = build_app(&state, &fonts);
        let mut h = Self {
            state,
            app,
            size: Size::new(WIDTH, HEIGHT),
        };
        h.frame();
        h
    }

    /// One shell frame: drain queued UI work, lay out, paint.
    pub fn frame(&mut self) {
        agg_gui::ui_thread::invoke_pending_actions();
        self.app.layout(self.size);
        let mut fb = Framebuffer::new(self.size.width as u32, self.size.height as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        self.app.paint(&mut ctx);
    }

    /// Whether widget `id` is in the tree and on screen.
    pub fn visible(&self, id: &str) -> bool {
        find_widget_by_id(self.app.root(), id).is_some()
            && find_widget_screen_rect(self.app.root(), id).is_some()
    }

    /// The on-screen rectangle of widget `id` (Y-up, as agg-gui reports it).
    pub fn rect(&self, id: &str) -> Rect {
        find_widget_screen_rect(self.app.root(), id)
            .unwrap_or_else(|| panic!("no visible widget {id:?}"))
    }

    /// The text of the `Label` with id `id`.
    pub fn label_text(&self, id: &str) -> String {
        find_widget_by_id(self.app.root(), id)
            .and_then(|w| w.as_any())
            .and_then(|a| a.downcast_ref::<Label>())
            .map(|l| l.text_str().to_string())
            .unwrap_or_else(|| panic!("no label {id:?}"))
    }

    /// Inspector property `name` of widget `id` (a `Button`'s `"label"`, say).
    pub fn property(&self, id: &str, name: &str) -> String {
        find_widget_by_id(self.app.root(), id)
            .unwrap_or_else(|| panic!("no widget {id:?}"))
            .properties()
            .into_iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("widget {id:?} has no property {name:?}"))
    }

    /// The tooltip text of widget `id`, if it has one.
    pub fn tooltip(&self, id: &str) -> Option<String> {
        find_widget_by_id(self.app.root(), id)
            .unwrap_or_else(|| panic!("no widget {id:?}"))
            .tooltip_text()
            .map(str::to_string)
    }

    /// Mark the widget instance `id` (through its tooltip slot, which the widgets used here
    /// leave empty) so [`Harness::is_marked`] can tell later whether it is still the same
    /// instance or was rebuilt. Address comparison would not do: a rebuilt widget can land
    /// at the freed address.
    pub fn mark(&mut self, id: &str) {
        find_widget_by_id_mut(self.app.root_mut(), id)
            .unwrap_or_else(|| panic!("no widget {id:?}"))
            .set_tooltip_text(Some(REBUILD_MARK.to_string()));
        assert!(self.is_marked(id), "widget {id:?} cannot hold a mark");
    }

    /// Whether widget `id` is the instance [`Harness::mark`] marked (not rebuilt since).
    pub fn is_marked(&self, id: &str) -> bool {
        self.tooltip(id).as_deref() == Some(REBUILD_MARK)
    }

    /// Click the centre of widget `id` (Y-down screen coordinates, as a shell sends).
    pub fn click(&mut self, id: &str) {
        let r = self.rect(id);
        let x = r.x + r.width / 2.0;
        let y = self.size.height - (r.y + r.height / 2.0);
        self.app.on_mouse_move(x, y);
        self.app
            .on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
        self.app
            .on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
        self.frame();
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Leave the thread's UI queue to whoever runs next on this test thread.
        agg_gui::ui_thread::unbind_current_thread();
    }
}
