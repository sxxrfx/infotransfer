use gpui::{AssetSource, SharedString, Styled, Svg, px, rgb, svg};
use std::borrow::Cow;

#[derive(Clone, Copy)]
pub enum Icon {
    General,
    Models,
    Voices,
    Advanced,
    About,
    Play,
    Pause,
    Stop,
    Settings,
    Close,
    Check,
    Plus,
    Minus,
    ChevronDown,
    Keyboard,
    Dictionary,
    Clipboard,
    Download,
    History,
    Search,
    SpeechSpinner,
}
impl Icon {
    fn path(self) -> &'static str {
        match self {
            Self::General => "icons/sliders-horizontal.svg",
            Self::Models => "icons/cpu.svg",
            Self::Voices => "icons/audio-lines.svg",
            Self::Advanced => "icons/cog.svg",
            Self::About => "icons/info.svg",
            Self::Play => "icons/play.svg",
            Self::Pause => "icons/pause.svg",
            Self::Stop => "icons/square.svg",
            Self::Settings => "icons/settings.svg",
            Self::Close => "icons/x.svg",
            Self::Check => "icons/check.svg",
            Self::Plus => "icons/plus.svg",
            Self::Minus => "icons/minus.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::Dictionary => "icons/book-open.svg",
            Self::Clipboard => "icons/clipboard.svg",
            Self::Download => "icons/download.svg",
            Self::History => "icons/history.svg",
            Self::Search => "icons/search.svg",
            Self::SpeechSpinner => "icons/speech-spinner.svg",
        }
    }
    pub fn element(self, size: f32) -> Svg {
        svg()
            .path(self.path())
            .text_color(rgb(0xf2f4f7))
            .size(px(size))
            .flex_shrink_0()
    }
    pub fn inherited(self, size: f32) -> InheritedIcon {
        InheritedIcon { icon: self, size }
    }
}
#[derive(gpui::IntoElement)]
pub struct InheritedIcon {
    icon: Icon,
    size: f32,
}
impl gpui::RenderOnce for InheritedIcon {
    fn render(self, window: &mut gpui::Window, _: &mut gpui::App) -> impl gpui::IntoElement {
        self.icon
            .element(self.size)
            .text_color(window.text_style().color)
    }
}
pub struct UiAssets;
impl AssetSource for UiAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        macro_rules! icons {($($name:literal),* $(,)?)=>{match path {$(
            concat!("icons/",$name,".svg")=>Some(Cow::Borrowed(include_bytes!(concat!("../assets/icons/",$name,".svg")).as_slice())),
        )* _=>None}}}
        Ok(icons!(
            "sliders-horizontal",
            "book-open",
            "cpu",
            "audio-lines",
            "cog",
            "info",
            "play",
            "pause",
            "square",
            "settings",
            "x",
            "check",
            "plus",
            "minus",
            "chevron-down",
            "keyboard",
            "clipboard",
            "download",
            "history",
            "search",
            "speech-spinner"
        ))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
