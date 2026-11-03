use super::*;
use gpui::{Anchor, KeyDownEvent, anchored, canvas, deferred};
use std::{cell::Cell, rc::Rc};

impl Desktop {
    pub(super) fn footer(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let snapshot = self.reader.snapshot();
        let choices: Vec<u32> = (0..VOICES.len() as u32)
            .filter(|id| self.reader.assets_for_voice(*id).ready())
            .collect();
        let keyboard_choices = choices.clone();
        let bounds = Rc::new(Cell::new(Bounds::<gpui::Pixels>::default()));
        let measured = bounds.clone();
        let open = self.footer_open;
        let mut menu = div()
            .id("footer-voices")
            .role(Role::Menu)
            .aria_label("Change voice")
            .track_focus(&self.footer_menu_focus)
            .w(px(240.))
            .max_h(window.viewport_size().height * 0.6)
            .overflow_y_scroll()
            .py_2()
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_lg()
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.footer_open = false;
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "down" if !keyboard_choices.is_empty() => {
                        this.footer_cursor = (this.footer_cursor + 1) % keyboard_choices.len()
                    }
                    "up" if !keyboard_choices.is_empty() => {
                        this.footer_cursor = (this.footer_cursor + keyboard_choices.len() - 1)
                            % keyboard_choices.len()
                    }
                    "enter" | "space"
                        if !keyboard_choices.is_empty()
                            && this.footer_menu_focus.is_focused(window) =>
                    {
                        this.settings.speech.voice =
                            keyboard_choices[this.footer_cursor.min(keyboard_choices.len() - 1)];
                        this.preferences_changed();
                        this.footer_open = false;
                        window.focus(&this.footer_focus, cx);
                    }
                    "escape" => {
                        this.footer_open = false;
                        window.focus(&this.footer_focus, cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        if choices.is_empty() {
            menu = menu.child(
                button("footer-download", "Choose voices")
                    .w_full()
                    .border_0()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = 1;
                        this.footer_open = false;
                        cx.notify();
                    })),
            );
        }
        for (index, voice) in choices.iter().copied().enumerate() {
            let descriptor = &VOICES[voice as usize];
            let selected = self.settings.speech.voice == voice;
            let inner = div()
                .id(("footer-voice", voice as usize))
                .role(Role::MenuItemRadio)
                .aria_label(descriptor.name)
                .aria_description(descriptor.info)
                .aria_toggled(if selected {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .focusable()
                .tab_stop(true)
                .w_full()
                .px_3()
                .py_2()
                .cursor_pointer()
                .when(selected, |view| view.bg(rgb(PINK).opacity(0.1)))
                .when(self.footer_cursor == index, |view| {
                    view.aria_active_descendant()
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(13.125))
                                        .line_height(px(18.75))
                                        .text_color(rgb(TEXT).opacity(0.8))
                                        .child(descriptor.name),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.25))
                                        .line_height(px(15.))
                                        .italic()
                                        .text_color(rgb(TEXT).opacity(0.4))
                                        .child(descriptor.info),
                                ),
                        )
                        .when(selected, |view| {
                            view.child(
                                div()
                                    .text_size(px(11.25))
                                    .text_color(rgb(PINK))
                                    .child("Active"),
                            )
                        }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.settings.speech.voice = voice;
                    this.preferences_changed();
                    this.footer_open = false;
                    window.focus(&this.footer_focus, cx);
                    cx.notify();
                }));
            menu = menu.child(HandyButton::menu(
                ("footer-motion", index).into(),
                inner,
                false,
            ));
        }
        let trigger = div()
            .id("voice-footer")
            .role(Role::Button)
            .track_focus(&self.footer_focus)
            .tab_stop(true)
            .aria_label("Change voice")
            .aria_expanded(self.footer_open)
            .relative()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .child(
                div()
                    .size(px(7.5))
                    .rounded_full()
                    .bg(rgb(if snapshot.model_ready {
                        0x22c55e
                    } else {
                        0x808080
                    })),
            )
            .child(format!(
                "{} · {:.2}×",
                VOICES[self.settings.speech.voice as usize].name, self.settings.speech.speed
            ))
            .child(motion::tween(
                "footer-chevron",
                Icon::ChevronDown
                    .element(11.25)
                    .text_color(rgb(TEXT).opacity(0.6)),
                if self.footer_open {
                    std::f32::consts::PI
                } else {
                    0.
                },
                150,
                false,
                |icon, angle| {
                    icon.with_transformation(gpui::Transformation::rotate(gpui::radians(angle)))
                },
            ))
            .child(
                canvas(move |rect, _, _| measured.set(rect), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.footer_open = !open;
                this.voice_menu = None;
                if this.footer_open {
                    this.footer_anchor = bounds.get().origin - point(px(0.), px(7.5));
                    this.footer_cursor = choices
                        .iter()
                        .position(|id| *id == this.settings.speech.voice)
                        .unwrap_or(0);
                    window.focus(&this.footer_menu_focus, cx);
                }
                cx.notify();
            }))
            .when(self.footer_open, |view| {
                view.child(
                    deferred(
                        anchored()
                            .anchor(Anchor::BottomLeft)
                            .position(self.footer_anchor)
                            .snap_to_window_with_margin(px(7.5))
                            .child(menu),
                    )
                    .priority(2),
                )
            });
        div()
            .h(px(38.5))
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(BORDER))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(11.25))
            .line_height(px(15.))
            .text_color(rgb(TEXT).opacity(0.6))
            .child(trigger)
            .child("v0.1.0")
    }
}
