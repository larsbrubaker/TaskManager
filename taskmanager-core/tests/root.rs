//! Headless test of the app's root widget: the production tree from `build_app`, laid out
//! and painted into a software framebuffer at the default window size with no window and
//! no GPU, as a shell would drive it.

use agg_gui::widget::{find_widget_by_id, find_widget_screen_rect};
use agg_gui::{Framebuffer, GfxCtx, Label, Size};
use taskmanager_core::ui::TITLE_ID;
use taskmanager_core::{build_app, install_theme_and_fonts, APP_TITLE};

const WIDTH: f64 = 1242.0;
const HEIGHT: f64 = 791.0;

#[test]
fn root_shows_the_product_title_in_the_top_left() {
    let fonts = install_theme_and_fonts(1.0).expect("bundled fonts load");
    let mut app = build_app(&fonts);
    let size = Size::new(WIDTH, HEIGHT);
    app.layout(size);
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);

    let text = find_widget_by_id(app.root(), TITLE_ID)
        .and_then(|w| w.as_any())
        .and_then(|a| a.downcast_ref::<Label>())
        .map(|l| l.text_str().to_string())
        .expect("the title label exists");
    assert_eq!(text, APP_TITLE);

    // Y-up screen rect: the title sits in the top-left quarter of the window.
    let rect = find_widget_screen_rect(app.root(), TITLE_ID).expect("the title is visible");
    assert!(
        rect.width > 0.0 && rect.height > 0.0,
        "title has a size: {rect:?}"
    );
    assert!(rect.x < WIDTH / 4.0, "title is at the left: {rect:?}");
    assert!(rect.y > HEIGHT * 0.75, "title is at the top: {rect:?}");
}
