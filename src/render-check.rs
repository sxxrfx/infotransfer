use anyhow::{Result, ensure};
use gpui::{Context, HeadlessAppContext, Render, Window, div, prelude::*, px, rgb, size};
use infotransfer::design::{Icon, UiAssets};
use std::sync::Arc;
#[allow(dead_code)]
mod ui;

struct Probe;
impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .bg(rgb(0x000000))
            .text_color(rgb(0xffffff))
            .text_size(px(28.))
            .p_4()
            .child(div().size(px(12.)).bg(rgb(0xff0000)))
            .child("READING")
            .child(
                Icon::General
                    .element(24.)
                    .text_color(rgb(0xffffff))
                    .absolute()
                    .right_2()
                    .bottom_2(),
            )
    }
}
fn main() -> Result<()> {
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(UiAssets),
        gpui_platform::current_headless_renderer,
    );
    let window = cx.open_window(size(px(260.), px(110.)), |_, cx| cx.new(|_| Probe))?;
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
    let image = cx.capture_screenshot(window.into())?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache/render-check.png");
    std::fs::create_dir_all(path.parent().unwrap())?;
    image.save(&path)?;
    let white = image
        .enumerate_pixels()
        .filter(|(x, _, pixel)| {
            *x < image.width() - 80
                && pixel[0] > 160
                && pixel[1] > 160
                && pixel[2] > 160
                && pixel[3] > 0
        })
        .count();
    let icons = image
        .enumerate_pixels()
        .filter(|(x, y, pixel)| {
            *x > image.width() - 80
                && *y > image.height() - 80
                && pixel[0] > 160
                && pixel[1] > 160
                && pixel[2] > 160
        })
        .count();
    let red = image
        .pixels()
        .filter(|pixel| pixel[0] > 200 && pixel[1] < 20 && pixel[2] < 20)
        .count();
    println!(
        "{}",
        serde_json::json!({"white_text_pixels":white,"icon_pixels":icons,"red_control_pixels":red,"screenshot":path})
    );
    ensure!(red >= 100, "Control shape did not render");
    ensure!(icons >= 50, "Lucide icon did not render");
    ensure!(
        white >= 100,
        "Text is invisible although control shapes render"
    );
    ui::capture_component_previews()?;
    Ok(())
}
