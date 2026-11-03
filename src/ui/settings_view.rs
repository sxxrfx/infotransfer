use super::*;

impl Desktop {
    fn general(&self, cx: &mut Context<Self>) -> Div {
        let mut content = div().flex().flex_col().gap_6();
        if !self.reader.snapshot().model_ready {
            let paths = self.reader.assets_for_voice(self.settings.speech.voice);
            let downloading = self
                .downloading
                .as_ref()
                .filter(|progress| progress.model == paths.model);
            let installed = paths.ready();
            let is_downloading = downloading.is_some();
            let status = if let Some(progress) = downloading {
                let (done, total) = (progress.done, progress.total);
                format!("Downloading voices · {}%", done * 100 / total.max(1))
            } else if installed {
                "Loading voices…".into()
            } else {
                "Voices not installed".into()
            };
            content = content.child(
                group().child(
                    row(&status).border_b_0().child(
                        button(
                            "download",
                            if downloading.is_some() {
                                "Cancel"
                            } else if installed {
                                "Load voices"
                            } else {
                                "Download voices"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !is_downloading && installed {
                                this.reader.retry_load();
                            } else {
                                this.download(cx);
                            }
                        })),
                    ),
                ),
            );
        }
        content = content.child(shortcuts::panel(
            native::activation_key(),
            self.settings.activation_key,
            cx.listener(
                |this, preference: &infotransfer::activation::KeyPreference, _, cx| {
                    this.cancel_capture();
                    this.settings.activation_key = *preference;
                    native::configure_activation(*preference);
                    this.writer.save(&this.settings);
                    cx.notify();
                },
            ),
        ));
        if self.settings.activation_key == infotransfer::activation::KeyPreference::Fn
            && !native::fn_unassigned()
        {
            content =
                content.child(div().text_size(px(12.)).text_color(rgb(0xe9b65a)).child(
                    "Fn may be assigned in macOS. Choose right Option to avoid a conflict.",
                ));
        }
        let allowed = native::ax_trusted();
        let access = group().child(
            row("Accessibility").border_b_0().child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("selection-access-status")
                            .role(Role::Status)
                            .aria_label(if allowed { "Allowed" } else { "Not allowed" })
                            .text_size(px(12.))
                            .text_color(rgb(TEXT).opacity(0.6))
                            .child(if allowed { "Allowed" } else { "Not allowed" }),
                    )
                    .child(
                        button("accessibility", "Open settings").on_click(cx.listener(
                            |_, _, _, cx| {
                                if native::ax_trusted() {
                                    native::access_settings();
                                } else {
                                    native::request_ax();
                                }
                                cx.notify();
                            },
                        )),
                    ),
            ),
        );
        content = content.child(div().child(section("Access")).child(access));
        content.child(
            div().child(section("Reading")).child(
                group()
                    .child(row("Clipboard").child(
                        button("read-clipboard", "Read clipboard").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.handle_native(
                                    native::Event {
                                        action: 2,
                                        pid: std::process::id() as i32,
                                        activation_ns: clock_ns(),
                                    },
                                    window,
                                    cx,
                                )
                            },
                        )),
                    ))
                    .child(self.speed_row(cx))
                    .child(self.setting_row(slider::Adjustment::SentencePause, cx))
                    .child(self.setting_row(slider::Adjustment::ParagraphPause, cx))
                    .child(self.setting_row(slider::Adjustment::Volume, cx))
                    .child(self.tone_row(cx))
                    .child(
                        row("Audio").border_b_0().child(
                            div()
                                .flex()
                                .gap_2()
                                .child(button("audio-preview", "Preview").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.preview_voice(this.settings.speech.voice, cx)
                                    },
                                )))
                                .child(button("audio-reset", "Reset").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.settings.speech.sound = Default::default();
                                        this.settings.speech.speed = 1.;
                                        this.preferences_changed();
                                        cx.notify();
                                    },
                                ))),
                        ),
                    ),
            ),
        )
    }

    fn tone_row(&self, cx: &mut Context<Self>) -> Div {
        let mut choices = div().id("speech-tone").flex().gap_1();
        for tone in infotransfer::sound::Tone::ALL {
            let selected = self.settings.speech.sound.tone == tone;
            choices = choices.child(
                tool_button(tone.label(), tone.label(), selected)
                    .role(Role::RadioButton)
                    .aria_toggled(if selected {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.speech.sound.tone = tone;
                        this.preferences_changed();
                        cx.notify();
                    })),
            );
        }
        row("Tone").child(choices.role(Role::RadioGroup).aria_label("Tone"))
    }

    fn advanced(&self, cx: &mut Context<Self>) -> Div {
        let saver = self.settings.speech.memory_saver;
        let toggle = div()
            .id("memory-saver")
            .role(Role::Switch)
            .focusable()
            .tab_stop(true)
            .aria_label("Memory saver")
            .aria_toggled(if saver {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .w(px(41.25))
            .h(px(22.5))
            .relative()
            .rounded_full()
            .bg(if saver {
                rgb(0x2f9e8f)
            } else {
                rgb(0x808080).opacity(0.2)
            })
            .cursor_pointer()
            .focus_visible(|style| style.border_1().border_color(rgb(TEXT)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings.speech.memory_saver = !this.settings.speech.memory_saver;
                this.preferences_changed();
                cx.notify();
            }))
            .child(motion::tween(
                "memory-saver-thumb",
                div()
                    .absolute()
                    .top(px(2.))
                    .left(px(2.))
                    .size(px(18.75))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(0xd1d5db))
                    .bg(rgb(0xffffff)),
                if saver { 18.75f32 } else { 0. },
                150,
                false,
                |view, offset| view.ml(px(offset)),
            ));
        let login = native::open_at_login();
        div().flex().flex_col().gap_6()
            .child(div().child(section("Startup")).child(group().child(row("Open at login").border_b_0().child(
                button("open-at-login", if login { "Turn off" } else { "Turn on" }).on_click(cx.listener(move |this, _, _, cx| {
                    if !native::set_open_at_login(!login) {
                        this.reader.notice("Open at login could not be changed. Move InfoTransfer to Applications and try again.".into());
                    }
                    cx.notify();
                })),
            ))))
            .child(div().child(section("Memory")).child(group().child(row("Memory saver").border_b_0().child(toggle)))
                .child(div().px_4().mt_2().text_size(px(12.)).line_height(px(18.)).text_color(rgb(TEXT).opacity(0.6))
                    .child("Unload voices after five idle minutes. The next reading starts more slowly.")))
            .child(div().child(section("Voices")).child(group().child(row("Speech assets").border_b_0().child(
                button("repair-voices", "Repair voices").on_click(cx.listener(|this, _, _, cx| this.download(cx))),
            ))))
    }
}

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tab != 1 {
            native::search_visible(false);
        }
        let mut navigation = div()
            .w(px(150.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .px_2()
            .child(
                div()
                    .h(px(72.323))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(rgb(ACCENT))
                    .child(Icon::Voices.element(23.).text_color(rgb(ACCENT)))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("InfoTransfer"),
                    ),
            )
            .child(div().h(px(1.)).bg(rgb(BORDER)).mb_2());
        for (index, (label, icon)) in [
            ("General", Icon::General),
            ("Voices", Icon::Models),
            ("Dictionary", Icon::Dictionary),
            ("Advanced", Icon::Advanced),
            ("About", Icon::About),
        ]
        .into_iter()
        .enumerate()
        {
            let active = self.tab == index;
            navigation = navigation.child(AppButton::sidebar(
                ("tab-motion", index).into(),
                div()
                    .id(("tab", index))
                    .role(Role::Button)
                    .focusable()
                    .tab_stop(true)
                    .aria_label(label)
                    .w_full()
                    .h(px(39.))
                    .mb_1()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(7.5))
                    .bg(if active {
                        rgb(0x24786d)
                    } else {
                        rgb(BG).opacity(0.)
                    })
                    .text_color(rgb(TEXT))
                    .opacity(if active { 1. } else { 0.85 })
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_size(px(13.125))
                    .line_height(px(18.75))
                    .cursor_pointer()
                    .when(!active, |view| view.hover(|style| style.opacity(1.)))
                    .focus_visible(|style| style.border_1().border_color(rgb(TEXT)))
                    .child(icon.element(24.).text_color(rgb(TEXT)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = index;
                        this.voice_menu = None;
                        this.footer_open = false;
                        cx.notify();
                    })),
                active,
            ));
        }
        let content = match self.tab {
            1 => self.voices(window, cx),
            2 => self.dictionary(cx),
            3 => self.advanced(cx),
            4 => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(section("About"))
                .child(
                    group()
                        .child(row("InfoTransfer").child("0.1.0"))
                        .child(row("License").border_b_0().child("Apache-2.0")),
                )
                .child(
                    button("source", "Source code")
                        .on_click(|_, _, cx| cx.open_url("https://github.com/sxxrfx/infotransfer")),
                )
                .child(
                    button("licenses", "Third-party notices").on_click(cx.listener(
                        |this, _, _, cx| cx.reveal_path(&this.paths.speech.join("PROVENANCE.txt")),
                    )),
                ),
            _ => self.general(cx),
        };
        let snapshot = self.reader.snapshot();
        let footer = self.footer(window, cx);
        div()
            .id("settings")
            .key_context("InfoTransferSettings")
            .role(Role::Application)
            .aria_label("InfoTransfer settings")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Tab, window, cx| {
                if !this.search_focused {
                    window.focus_next(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                if !this.search_focused {
                    window.focus_prev(cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(".SystemUIFont")
            .text_size(px(15.))
            .line_height(px(24.))
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pl(px(84.))
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT).opacity(0.6))
                    .child("InfoTransfer"),
            )
            .child(
                div().flex_1().min_h_0().flex().child(navigation).child(
                    div()
                        .id("settings-content")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .p_4()
                        .child(content)
                        .when_some(snapshot.notice, |view, notice| {
                            view.child(
                                div()
                                    .mt_4()
                                    .flex_shrink_0()
                                    .p_3()
                                    .border_1()
                                    .border_color(rgb(ACCENT))
                                    .rounded_md()
                                    .child(notice),
                            )
                        }),
                ),
            )
            .child(footer)
    }
}
