use super::*;
use gpui::HeadlessAppContext;
use infotransfer::activation::{ActivationKey, KeyPreference};

enum Content {
    Shortcuts(ActivationKey),
    Player(Phase, Option<&'static str>),
}
struct Probe(Content);
impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match self.0 {
            Content::Shortcuts(key) => div()
                .size_full()
                .bg(rgb(BG))
                .text_color(rgb(TEXT))
                .font_family(".SystemUIFont")
                .p(px(20.))
                .child(shortcuts::panel(key, KeyPreference::Auto, |_, _, _| {}))
                .into_any_element(),
            Content::Player(phase, notice) => {
                let bars = [0.1, 0.4, 0.7, 0.3, 1., 0.85, 0.5, 0.2, 0.4].into();
                player::visual(phase, false, notice, &bars, true)
            }
        }
    }
}
pub(crate) fn capture_component_previews() -> anyhow::Result<()> {
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(UiAssets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| cx.set_reduce_motion(true));
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache/ui-proof");
    std::fs::create_dir_all(&directory)?;
    for (name, dimensions, content) in [
        (
            "shortcuts-fn",
            size(px(480.), px(260.)),
            Content::Shortcuts(ActivationKey::Fn),
        ),
        (
            "shortcuts-option",
            size(px(480.), px(260.)),
            Content::Shortcuts(ActivationKey::RightOption),
        ),
        (
            "player-reading",
            size(px(200.), px(28.)),
            Content::Player(Phase::Reading, None),
        ),
        (
            "player-preparing",
            size(px(200.), px(28.)),
            Content::Player(Phase::Preparing, None),
        ),
        (
            "player-empty",
            size(px(200.), px(28.)),
            Content::Player(Phase::Idle, Some("No text selected")),
        ),
    ] {
        let window = cx.open_window(dimensions, |_, cx| cx.new(|_| Probe(content)))?;
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
        let image = cx.capture_screenshot(window.into())?;
        image.save(directory.join(format!("{name}.png")))?;
    }
    println!("GPUI component captures: {}", directory.display());
    Ok(())
}
