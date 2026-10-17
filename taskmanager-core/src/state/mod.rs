//! Application state shared by the widget tree, the shells and the tests.
//!
//! The UI runs on one thread (the browser main thread on wasm), so state lives in one `Rc`:
//! widgets hold clones of [`AppState`] and read it when they are (re)built, and tests drive
//! the same state directly. One counter per kind of change tells the widget tree what to
//! rebuild, so a change rebuilds only the parts that show it:
//!
//! - [`AppState::page_version`]: the page or the processor brand (which names a page)
//!   changed; the page host, the title row and the sidebar rows watch it.
//! - [`AppState::sidebar_version`]: the sidebar was shown or hidden; only the title row's
//!   toggle (its tooltip says which way it goes) watches it. The sidebar itself is shown or
//!   hidden by a `Conditional` on the shared flag, without any rebuild.
//! - [`AppState::status_version`]: the status line changed; the status bar watches it.
//! - [`AppState::theme_version`]: a look was installed; the whole window row, built with
//!   theme colours, watches it.
//!
//! Every change also asks agg-gui for a frame, since the shells repaint only on demand.
//!
//! What lives here for now: the current [`Page`], whether the sidebar is shown, the
//! [`ProcessorBrand`] the platform reported (it names the Processor page) and the status
//! line. The theme (choice, OS appearance, installing the look) is in `state/theme.rs`.

mod theme;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::page::{Page, ProcessorBrand};
use crate::APP_TITLE;

/// How the status bar's dot is coloured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusLevel {
    /// Nothing to report on (a dim dot).
    #[default]
    Neutral,
    /// Everything that should be measured is being measured (green).
    Healthy,
    /// Some readings are unavailable (orange).
    Degraded,
}

/// The status bar's left side: a dot and a line of text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub level: StatusLevel,
    pub text: String,
}

/// See the module docs.
#[derive(Clone)]
pub struct AppState {
    inner: Rc<Inner>,
}

struct Inner {
    page: Cell<Page>,
    brand: Cell<ProcessorBrand>,
    /// Shared with the sidebar's `Conditional`, which shows or hides it without a rebuild.
    sidebar_visible: Rc<Cell<bool>>,
    status: RefCell<StatusLine>,
    page_version: Cell<u64>,
    sidebar_version: Cell<u64>,
    status_version: Cell<u64>,
    theme: theme::ThemeState,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Summary, sidebar shown, Match System, generic processor; status "TaskManager
    /// <version>" with a neutral dot.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                page: Cell::new(Page::default()),
                brand: Cell::new(ProcessorBrand::default()),
                sidebar_visible: Rc::new(Cell::new(true)),
                status: RefCell::new(StatusLine {
                    level: StatusLevel::Neutral,
                    text: format!("{APP_TITLE} {}", env!("CARGO_PKG_VERSION")),
                }),
                page_version: Cell::new(0),
                sidebar_version: Cell::new(0),
                status_version: Cell::new(0),
                theme: theme::ThemeState::default(),
            }),
        }
    }

    /// Bumped when the page or the processor brand changes.
    pub fn page_version(&self) -> u64 {
        self.inner.page_version.get()
    }

    /// Bumped when the sidebar is shown or hidden.
    pub fn sidebar_version(&self) -> u64 {
        self.inner.sidebar_version.get()
    }

    /// Bumped when the status line changes.
    pub fn status_version(&self) -> u64 {
        self.inner.status_version.get()
    }

    /// Record a visible change: bump `version` and ask for a frame.
    fn changed(version: &Cell<u64>) {
        version.set(version.get().wrapping_add(1));
        agg_gui::animation::request_draw();
    }

    /// The page shown.
    pub fn page(&self) -> Page {
        self.inner.page.get()
    }

    /// Show `page`.
    pub fn set_page(&self, page: Page) {
        if self.page() != page {
            self.inner.page.set(page);
            Self::changed(&self.inner.page_version);
        }
    }

    /// What kind of CPU the platform reported.
    pub fn processor_brand(&self) -> ProcessorBrand {
        self.inner.brand.get()
    }

    /// The platform reports its CPU (names the Processor page).
    pub fn set_processor_brand(&self, brand: ProcessorBrand) {
        if self.processor_brand() != brand {
            self.inner.brand.set(brand);
            Self::changed(&self.inner.page_version);
        }
    }

    /// The current page's title.
    pub fn page_title(&self) -> &'static str {
        self.page().title(self.processor_brand())
    }

    /// Whether the sidebar is shown.
    pub fn sidebar_visible(&self) -> bool {
        self.inner.sidebar_visible.get()
    }

    /// Show or hide the sidebar.
    pub fn set_sidebar_visible(&self, visible: bool) {
        if self.sidebar_visible() != visible {
            self.inner.sidebar_visible.set(visible);
            Self::changed(&self.inner.sidebar_version);
        }
    }

    /// Hide a shown sidebar, show a hidden one (the title row's toggle).
    pub fn toggle_sidebar(&self) {
        self.set_sidebar_visible(!self.sidebar_visible());
    }

    /// The cell the sidebar's `Conditional` reads.
    pub(crate) fn sidebar_visibility(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.inner.sidebar_visible)
    }

    /// The status bar's dot and text.
    pub fn status(&self) -> StatusLine {
        self.inner.status.borrow().clone()
    }

    /// Replace the status bar's dot and text.
    pub fn set_status(&self, status: StatusLine) {
        if *self.inner.status.borrow() != status {
            *self.inner.status.borrow_mut() = status;
            Self::changed(&self.inner.status_version);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeChoice;

    #[test]
    fn starts_on_summary_with_the_sidebar_and_a_neutral_status() {
        let state = AppState::new();
        assert_eq!(state.page(), Page::Summary);
        assert!(state.sidebar_visible());
        assert_eq!(state.theme(), ThemeChoice::System);
        let status = state.status();
        assert_eq!(status.level, StatusLevel::Neutral);
        assert_eq!(
            status.text,
            format!("TaskManager {}", env!("CARGO_PKG_VERSION"))
        );
    }

    /// The four counters, in a tuple to compare before and after a change.
    fn versions(state: &AppState) -> (u64, u64, u64, u64) {
        (
            state.page_version(),
            state.sidebar_version(),
            state.status_version(),
            state.theme_version(),
        )
    }

    #[test]
    fn each_change_bumps_only_its_own_version_and_no_ops_bump_none() {
        let state = AppState::new();
        let (p, sb, st, t) = versions(&state);
        state.set_page(Page::Summary);
        assert_eq!(versions(&state), (p, sb, st, t), "same page is no change");

        state.set_page(Page::Users);
        assert_eq!(versions(&state), (p + 1, sb, st, t), "page");
        state.set_processor_brand(ProcessorBrand::AppleSilicon);
        assert_eq!(versions(&state), (p + 2, sb, st, t), "brand");

        state.toggle_sidebar();
        assert!(!state.sidebar_visible());
        assert_eq!(versions(&state), (p + 2, sb + 1, st, t), "sidebar");
        state.set_sidebar_visible(false);
        assert_eq!(versions(&state), (p + 2, sb + 1, st, t), "already hidden");

        let status = StatusLine {
            level: StatusLevel::Healthy,
            text: "ok".into(),
        };
        state.set_status(status.clone());
        assert_eq!(versions(&state), (p + 2, sb + 1, st + 1, t), "status");
        state.set_status(status);
        assert_eq!(versions(&state), (p + 2, sb + 1, st + 1, t), "same status");
    }

    #[test]
    fn processor_title_follows_the_reported_brand() {
        let state = AppState::new();
        state.set_page(Page::Processor);
        assert_eq!(state.page_title(), "Processor");
        state.set_processor_brand(ProcessorBrand::AppleSilicon);
        assert_eq!(state.page_title(), "Apple Silicon");
    }
}
