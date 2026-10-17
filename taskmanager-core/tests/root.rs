//! Headless test of the app's root widget: the production tree from `build_app`, laid out
//! and painted into a software framebuffer at the default window size with no window and
//! no GPU, as a shell would drive it. The chrome's parts sit where PLAN §2 puts them.

mod common;

use common::{Harness, HEIGHT, WIDTH};
use taskmanager_core::ui::chrome::{
    page_content_id, BAR_HEIGHT, LOGO_ID, PAGE_TITLE_ID, SIDEBAR_ID, SIDEBAR_WIDTH, STATUS_TEXT_ID,
    TITLE_ROW_HEIGHT,
};

#[test]
fn chrome_frames_the_page_like_the_reference_window() {
    let h = Harness::new();

    // Y-up screen rects. The sidebar runs down the whole left edge.
    let sidebar = h.rect(SIDEBAR_ID);
    assert_eq!(sidebar.x, 0.0, "{sidebar:?}");
    assert!((sidebar.width - SIDEBAR_WIDTH).abs() < 0.5, "{sidebar:?}");
    assert!((sidebar.height - HEIGHT).abs() < 0.5, "{sidebar:?}");

    // The page title sits top-left of the content area.
    assert_eq!(h.label_text(PAGE_TITLE_ID), "Summary");
    let title = h.rect(PAGE_TITLE_ID);
    assert!(
        title.x > SIDEBAR_WIDTH && title.x < SIDEBAR_WIDTH + 80.0,
        "{title:?}"
    );
    assert!(
        title.y > HEIGHT - TITLE_ROW_HEIGHT,
        "title in the title row: {title:?}"
    );

    // The status line sits in the bottom bar, the logo above the sidebar footer.
    let status = h.rect(STATUS_TEXT_ID);
    assert!(
        status.x > SIDEBAR_WIDTH && status.y < BAR_HEIGHT,
        "{status:?}"
    );
    let logo = h.rect(LOGO_ID);
    assert!(logo.y > BAR_HEIGHT && logo.y < HEIGHT / 3.0, "{logo:?}");

    // The page host fills the rest of the content area.
    let page = h.rect(&page_content_id("summary"));
    assert!((page.x - SIDEBAR_WIDTH).abs() < 0.5, "{page:?}");
    assert!(
        (page.width - (WIDTH - SIDEBAR_WIDTH)).abs() < 0.5,
        "{page:?}"
    );
    assert!(
        page.height > HEIGHT - TITLE_ROW_HEIGHT - BAR_HEIGHT - 4.0,
        "{page:?}"
    );
}
