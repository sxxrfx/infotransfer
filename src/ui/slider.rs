use super::*;
use gpui::{AccessibleAction, MouseButton, MouseDownEvent, MouseMoveEvent, canvas};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Adjustment {
    Speed,
    SentencePause,
    ParagraphPause,
    Volume,
}
impl Adjustment {
    fn limits(self) -> (f32, f32, f32) {
        match self {
            Self::Speed => (0.75, 2., 0.05),
            Self::SentencePause => (0., 1000., 50.),
            Self::ParagraphPause => (0., 2000., 50.),
            Self::Volume => (0.25, 1.5, 0.05),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Speed => "Speed",
            Self::SentencePause => "Sentence pause",
            Self::ParagraphPause => "Paragraph pause",
            Self::Volume => "Volume",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Speed => "reading-speed",
            Self::SentencePause => "sentence-pause",
            Self::ParagraphPause => "paragraph-pause",
            Self::Volume => "speech-volume",
        }
    }
    fn value(self, prefs: &infotransfer::reader::Preferences) -> f32 {
        match self {
            Self::Speed => prefs.speed,
            Self::SentencePause => prefs.sound.sentence_pause_ms as f32,
            Self::ParagraphPause => prefs.sound.paragraph_pause_ms as f32,
            Self::Volume => prefs.sound.volume,
        }
    }
    fn format(self, value: f32) -> String {
        match self {
            Self::Speed => format!("{value:.2}×"),
            Self::Volume => format!("{:.0}%", value * 100.),
            _ => format!("{value:.0} ms"),
        }
    }
}
impl Desktop {
    fn adjust(&mut self, setting: Adjustment, delta: f32, cx: &mut Context<Self>) {
        self.set_adjustment(setting, setting.value(&self.settings.speech) + delta, cx);
    }
    fn set_adjustment(&mut self, setting: Adjustment, value: f32, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        let (min, max, step) = setting.limits();
        let value = ((value / step).round() * step).clamp(min, max);
        match setting {
            Adjustment::Speed => self.settings.speech.speed = value,
            Adjustment::SentencePause => {
                self.settings.speech.sound.sentence_pause_ms = value as u32
            }
            Adjustment::ParagraphPause => {
                self.settings.speech.sound.paragraph_pause_ms = value as u32
            }
            Adjustment::Volume => self.settings.speech.sound.volume = value,
        }
        self.preferences_changed();
        cx.notify();
    }
    pub(super) fn speed_row(&self, cx: &mut Context<Self>) -> Div {
        self.setting_row(Adjustment::Speed, cx)
    }
    pub(super) fn setting_row(&self, setting: Adjustment, cx: &mut Context<Self>) -> Div {
        let value = setting.value(&self.settings.speech);
        let (min, max, step) = setting.limits();
        let progress = (value - min) / (max - min);
        let bounds = Rc::new(Cell::new(Bounds::<gpui::Pixels>::default()));
        let measured = bounds.clone();
        let down_bounds = bounds.clone();
        let increment = cx.entity().downgrade();
        let decrement = increment.clone();
        let set = increment.clone();
        let slider = div()
            .id(setting.id())
            .role(Role::Slider)
            .focusable()
            .tab_stop(true)
            .aria_label(setting.label())
            .aria_description(match setting {
                Adjustment::SentencePause | Adjustment::ParagraphPause => {
                    "Minimum quiet time. Existing pauses may be longer."
                }
                Adjustment::Volume => "Volume above 100% may engage peak protection.",
                Adjustment::Speed => "Reading speed without changing pitch.",
            })
            .aria_value(setting.format(value))
            .aria_numeric_value(value as f64)
            .aria_numeric_value_step(step as f64)
            .aria_min_numeric_value(min as f64)
            .aria_max_numeric_value(max as f64)
            .aria_orientation(gpui::Orientation::Horizontal)
            .relative()
            .flex_1()
            .h(px(22.5))
            .cursor_pointer()
            .focus_visible(|style| style.rounded_lg().border_1().border_color(rgb(ACCENT)))
            .child(
                canvas(move |rect, _, _| measured.set(rect), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top(px(7.5))
                    .w_full()
                    .h(px(7.5))
                    .rounded_lg()
                    .bg(rgb(0x808080).opacity(0.2))
                    .child(
                        div()
                            .w(gpui::relative(progress))
                            .h_full()
                            .rounded_lg()
                            .bg(rgb(0x2f9e8f)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(3.25))
                    .left(gpui::relative(progress))
                    .ml(px(-24. * progress))
                    .w(px(24.))
                    .h(px(16.))
                    .rounded_full()
                    .bg(rgb(0xffffff)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.slider_dragging = Some(setting);
                    let bounds = down_bounds.get();
                    if bounds.size.width > px(0.) {
                        this.set_adjustment(
                            setting,
                            min + ((event.position.x - bounds.left()) / bounds.size.width)
                                .clamp(0., 1.)
                                * (max - min),
                            cx,
                        );
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let bounds = bounds.get();
                if this.slider_dragging == Some(setting) && bounds.size.width > px(0.) {
                    this.set_adjustment(
                        setting,
                        min + ((event.position.x - bounds.left()) / bounds.size.width)
                            .clamp(0., 1.)
                            * (max - min),
                        cx,
                    );
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.slider_dragging = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.slider_dragging = None),
            )
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "left" | "down" => this.adjust(setting, -step, cx),
                    "right" | "up" => this.adjust(setting, step, cx),
                    "home" => this.set_adjustment(setting, min, cx),
                    "end" => this.set_adjustment(setting, max, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_a11y_action(AccessibleAction::Increment, move |_, _, cx| {
                let _ = increment.update(cx, |this, cx| this.adjust(setting, step, cx));
            })
            .on_a11y_action(AccessibleAction::Decrement, move |_, _, cx| {
                let _ = decrement.update(cx, |this, cx| this.adjust(setting, -step, cx));
            })
            .on_a11y_action(AccessibleAction::SetValue, move |data, _, cx| {
                let value = match data {
                    Some(accesskit::ActionData::NumericValue(value)) => Some(*value as f32),
                    Some(accesskit::ActionData::Value(value)) => {
                        let text = value.trim();
                        let parsed = text
                            .trim_end_matches('×')
                            .trim_end_matches('%')
                            .trim_end_matches("ms")
                            .trim()
                            .parse::<f32>()
                            .ok();
                        parsed.map(|value| {
                            if setting == Adjustment::Volume && text.ends_with('%') {
                                value / 100.
                            } else {
                                value
                            }
                        })
                    }
                    _ => None,
                };
                if let Some(value) = value.filter(|value| value.is_finite()) {
                    let _ = set.update(cx, |this, cx| this.set_adjustment(setting, value, cx));
                }
            });
        row(setting.label()).child(
            div()
                .w(px(200.))
                .h(px(22.5))
                .flex()
                .items_center()
                .gap_1()
                .child(slider)
                .child(
                    div()
                        .w(px(60.))
                        .text_right()
                        .text_size(px(13.125))
                        .line_height(px(18.75))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(TEXT).opacity(0.9))
                        .child(setting.format(value)),
                ),
        )
    }
}
