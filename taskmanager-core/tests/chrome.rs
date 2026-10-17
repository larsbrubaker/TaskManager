//! Headless tests of the window chrome (PLAN §2) through agg-gui's input path: the sidebar
//! rows switch pages and the title follows, the footer's Settings button opens Settings,
//! the sun / moon button flips Light / Dark (and Match System follows the OS), the title
//! row's toggle hides and shows the sidebar; each change rebuilds only the parts that show
//! it; plus the page table itself (unique ids and icons) and `TASKMANAGER_PAGE` ids.

mod common;

use std::collections::HashSet;

use agg_gui::current_visuals;
use common::Harness;
use taskmanager_core::state::{StatusLevel, StatusLine};
use taskmanager_core::ui::chrome::title_row::{SIDEBAR_HIDE_TIP, SIDEBAR_SHOW_TIP};
use taskmanager_core::ui::chrome::{
    nav_row_id, page_content_id, PAGE_TITLE_ID, SETTINGS_BUTTON_ID, SIDEBAR_ID, SIDEBAR_TOGGLE_ID,
    SIDEBAR_WIDTH, STATUS_TEXT_ID, THEME_BUTTON_ID,
};
use taskmanager_core::{AppState, Page, ProcessorBrand, ThemeChoice};

#[test]
fn clicking_each_sidebar_row_selects_its_page_and_title() {
    let mut h = Harness::new();
    // Last to first, so every click is a change (Summary is the start page).
    for page in Page::SIDEBAR.iter().rev() {
        h.click(&nav_row_id(page.id()));
        assert_eq!(h.state.page(), *page);
        assert_eq!(
            h.label_text(PAGE_TITLE_ID),
            page.title(ProcessorBrand::Generic)
        );
        assert!(h.visible(&page_content_id(page.id())), "{page:?} content");
    }
}

#[test]
fn settings_button_opens_settings() {
    let mut h = Harness::new();
    h.click(SETTINGS_BUTTON_ID);
    assert_eq!(h.state.page(), Page::Settings);
    assert_eq!(h.label_text(PAGE_TITLE_ID), "Settings");
    assert!(h.visible(&page_content_id("settings")));
}

#[test]
fn processor_row_is_named_after_the_reported_chip() {
    let state = AppState::new();
    state.set_processor_brand(ProcessorBrand::AppleSilicon);
    let mut h = Harness::with_state(state);
    h.click(&nav_row_id(Page::Processor.id()));
    assert_eq!(h.label_text(PAGE_TITLE_ID), "Apple Silicon");
}

#[test]
fn theme_button_flips_light_and_dark() {
    let state = AppState::new();
    state.set_theme(ThemeChoice::Light);
    let mut h = Harness::with_state(state);
    assert!(!current_visuals().is_dark());
    assert_eq!(h.property(THEME_BUTTON_ID, "label"), "Dark mode");
    h.click(THEME_BUTTON_ID);
    assert_eq!(h.state.theme(), ThemeChoice::Dark);
    assert!(current_visuals().is_dark());
    // The chrome was rebuilt for the new look: the button offers the way back.
    assert_eq!(h.property(THEME_BUTTON_ID, "label"), "Light mode");
    assert_eq!(
        h.tooltip(THEME_BUTTON_ID).as_deref(),
        Some("Switch to the light theme")
    );
    h.click(THEME_BUTTON_ID);
    assert_eq!(h.state.theme(), ThemeChoice::Light);
    assert!(!current_visuals().is_dark());
    assert_eq!(h.property(THEME_BUTTON_ID, "label"), "Dark mode");
    assert_eq!(
        h.tooltip(THEME_BUTTON_ID).as_deref(),
        Some("Switch to the dark theme")
    );
}

#[test]
fn match_system_rebuilds_the_chrome_when_the_os_turns_dark() {
    let state = AppState::new();
    state.set_system_dark(Some(false));
    let mut h = Harness::with_state(state);
    assert_eq!(h.state.theme(), ThemeChoice::System);
    assert!(!current_visuals().is_dark());
    h.mark(&page_content_id("summary"));
    h.state.set_system_dark(Some(true));
    h.frame();
    assert!(current_visuals().is_dark());
    assert!(
        !h.is_marked(&page_content_id("summary")),
        "the chrome was rebuilt for the dark look"
    );
    assert_eq!(h.property(THEME_BUTTON_ID, "label"), "Light mode");
}

#[test]
fn a_status_change_rebuilds_only_the_status_bar() {
    let mut h = Harness::new();
    let page = page_content_id("summary");
    let row = nav_row_id("summary");
    for id in [page.as_str(), PAGE_TITLE_ID, row.as_str(), STATUS_TEXT_ID] {
        h.mark(id);
    }
    h.state.set_status(StatusLine {
        level: StatusLevel::Healthy,
        text: "All sensors read".into(),
    });
    h.frame();
    assert_eq!(h.label_text(STATUS_TEXT_ID), "All sensors read");
    assert!(!h.is_marked(STATUS_TEXT_ID), "status bar rebuilt");
    assert!(h.is_marked(&page), "page host kept");
    assert!(h.is_marked(PAGE_TITLE_ID), "title row kept");
    assert!(h.is_marked(&row), "sidebar rows kept");
}

#[test]
fn the_sidebar_toggle_rebuilds_only_its_own_button() {
    let mut h = Harness::new();
    let page = page_content_id("summary");
    let row = nav_row_id("summary");
    for id in [page.as_str(), PAGE_TITLE_ID, row.as_str(), STATUS_TEXT_ID] {
        h.mark(id);
    }
    assert_eq!(
        h.tooltip(SIDEBAR_TOGGLE_ID).as_deref(),
        Some(SIDEBAR_HIDE_TIP)
    );
    h.click(SIDEBAR_TOGGLE_ID);
    assert!(!h.state.sidebar_visible());
    assert_eq!(
        h.tooltip(SIDEBAR_TOGGLE_ID).as_deref(),
        Some(SIDEBAR_SHOW_TIP),
        "the tooltip follows without rebuilding the page"
    );
    for id in [page.as_str(), PAGE_TITLE_ID, STATUS_TEXT_ID] {
        assert!(h.is_marked(id), "{id} kept while hiding");
    }
    h.click(SIDEBAR_TOGGLE_ID);
    assert_eq!(
        h.tooltip(SIDEBAR_TOGGLE_ID).as_deref(),
        Some(SIDEBAR_HIDE_TIP)
    );
    for id in [page.as_str(), PAGE_TITLE_ID, row.as_str(), STATUS_TEXT_ID] {
        assert!(h.is_marked(id), "{id} kept after showing again");
    }
}

#[test]
fn a_page_change_rebuilds_the_page_host_title_and_rows_but_not_the_status_bar() {
    let mut h = Harness::new();
    let page = page_content_id("summary");
    let row = nav_row_id("summary");
    for id in [page.as_str(), PAGE_TITLE_ID, row.as_str(), STATUS_TEXT_ID] {
        h.mark(id);
    }
    // The brand names a page, so it counts as a page change (and keeps the page's ids).
    h.state.set_processor_brand(ProcessorBrand::AppleSilicon);
    h.frame();
    assert!(!h.is_marked(&page), "page host rebuilt");
    assert!(!h.is_marked(PAGE_TITLE_ID), "title row rebuilt");
    assert!(!h.is_marked(&row), "sidebar rows rebuilt");
    assert!(h.is_marked(STATUS_TEXT_ID), "status bar kept");

    h.mark(&page);
    h.click(&nav_row_id(Page::Users.id()));
    assert!(h.visible(&page_content_id(Page::Users.id())));
    assert!(h.is_marked(STATUS_TEXT_ID), "status bar kept");
    h.click(&nav_row_id(Page::Summary.id()));
    assert!(!h.is_marked(&page), "a fresh page host for Summary");
}

#[test]
fn sidebar_toggle_hides_and_shows_the_sidebar() {
    let mut h = Harness::new();
    assert!(h.visible(SIDEBAR_ID));
    h.click(SIDEBAR_TOGGLE_ID);
    assert!(!h.state.sidebar_visible());
    assert!(!h.visible(SIDEBAR_ID), "sidebar hidden");
    // The content takes the whole width.
    assert!(h.rect(PAGE_TITLE_ID).x < SIDEBAR_WIDTH / 2.0);
    h.click(SIDEBAR_TOGGLE_ID);
    assert!(h.state.sidebar_visible());
    assert!(h.visible(SIDEBAR_ID), "sidebar shown again");
    assert!(h.rect(PAGE_TITLE_ID).x > SIDEBAR_WIDTH);
}

#[test]
fn every_page_has_a_unique_id_and_icon() {
    let ids: HashSet<&str> = Page::ALL.iter().map(|p| p.id()).collect();
    let icons: HashSet<char> = Page::ALL.iter().map(|p| p.icon()).collect();
    assert_eq!(ids.len(), Page::ALL.len(), "duplicate page id");
    assert_eq!(icons.len(), Page::ALL.len(), "duplicate page icon");
    for id in &ids {
        assert!(
            id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "id {id:?} is not kebab-case"
        );
    }
}

#[test]
fn page_ids_parse_back_to_their_pages() {
    for page in Page::ALL {
        assert_eq!(Page::from_id(page.id()), Some(page));
    }
    assert_eq!(Page::from_id("Startup-Apps"), Some(Page::StartupApps));
    assert_eq!(Page::from_id(" services "), Some(Page::Services));
    assert_eq!(Page::from_id("pro"), None);
    assert_eq!(Page::from_id(""), None);
    assert!(Page::id_list().starts_with("summary, performance"));
}
