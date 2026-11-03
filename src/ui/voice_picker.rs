use super::*;
use gpui::{
    Anchor, Animation, AnimationExt, AnyElement, KeyDownEvent, RenderOnce, anchored, canvas,
    deferred,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VoiceMenu {
    Filter,
    Sort,
}

struct RatingTip(SharedString);

#[derive(IntoElement)]
struct DownloadMeter {
    id: usize,
    fraction: f32,
    scale: f32,
}
impl RenderOnce for DownloadMeter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        download_progress(self.id, self.fraction, self.scale, cx)
    }
}

fn download_progress(id: usize, fraction: f32, scale: f32, cx: &App) -> AnyElement {
    let fill = div().h_full().rounded_full().bg(rgb(ACCENT));
    let fill = if fraction >= 1. {
        let fill = fill.w_full();
        if cx.reduce_motion() {
            fill.into_any_element()
        } else {
            fill.with_animation(
                ("voice-preparing", id),
                Animation::new(std::time::Duration::from_secs(2)).repeat_synced(),
                |view, phase| {
                    let phase = if phase < 0.5 {
                        phase * 2.
                    } else {
                        (1. - phase) * 2.
                    };
                    view.opacity(1. - 0.5 * motion::cubic_bezier(phase, [0.4, 0., 0.6, 1.]))
                },
            )
            .into_any_element()
        }
    } else {
        motion::tween(
            ("voice-progress", id),
            fill,
            fraction,
            300,
            false,
            |view, fraction| view.w(gpui::relative(fraction)),
        )
        .into_any_element()
    };
    div()
        .w_full()
        .h(px(5.625 * scale))
        .rounded_full()
        .bg(rgb(0x808080).opacity(0.2))
        .overflow_hidden()
        .child(fill)
        .into_any_element()
}

impl Render for RatingTip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tip = div()
            .w(px(200.))
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(0x808080).opacity(0.8))
            .shadow_lg()
            .text_color(rgb(TEXT))
            .font_family(".SystemUIFont")
            .text_size(px(11.25))
            .line_height(px(15.))
            .child(self.0.clone());
        if cx.reduce_motion() {
            tip.into_any_element()
        } else {
            tip.with_animation(
                "rating-tip",
                Animation::new(std::time::Duration::from_millis(150)),
                |view, progress| view.opacity(motion::cubic_bezier(progress, [0.4, 0., 0.2, 1.])),
            )
            .into_any_element()
        }
    }
}

fn indicator(
    id: (&'static str, usize),
    label: &'static str,
    value: String,
    fill: Option<f32>,
    detail: String,
    scale: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(format!("{label}: {value}"))
        .aria_description(detail.clone())
        .flex()
        .items_center()
        .gap(px(7.5 * scale))
        .text_size(px(11.25 * scale))
        .line_height(px(15. * scale))
        .child(
            div()
                .w(px(90. * scale))
                .text_right()
                .text_color(rgb(TEXT).opacity(0.6))
                .child(if label == "Quality" && value == "Unrated" {
                    "Quality unrated".into()
                } else {
                    format!("{label} · {value}")
                }),
        )
        .child(
            div()
                .w(px(60. * scale))
                .h(px(5.625 * scale))
                .rounded_full()
                .bg(rgb(0x2c333c))
                .when_some(fill, |track, fill| {
                    track.child(
                        div()
                            .w(px(60. * scale * fill.clamp(0., 1.)))
                            .h_full()
                            .rounded_full()
                            .bg(rgb(0x5ec8b8)),
                    )
                }),
        )
        .tooltip_show_delay(std::time::Duration::ZERO)
        .tooltip(move |_, cx| cx.new(|_| RatingTip(detail.clone().into())).into())
}

impl Desktop {
    fn choose_voice_menu(
        &mut self,
        menu: VoiceMenu,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match menu {
            VoiceMenu::Filter => self.voice_filter = VoiceFilter::ALL[index],
            VoiceMenu::Sort => self.voice_sort = VoiceSort::ALL[index],
        }
        self.voice_menu = None;
        window.focus(
            match menu {
                VoiceMenu::Filter => &self.voice_filter_focus,
                VoiceMenu::Sort => &self.voice_sort_focus,
            },
            cx,
        );
        cx.notify();
    }

    fn voice_menu_button(&self, menu: VoiceMenu, cx: &mut Context<Self>) -> AppButton {
        let (id, label) = match menu {
            VoiceMenu::Filter => ("voice-filter", self.voice_filter.short_label().to_owned()),
            VoiceMenu::Sort => ("voice-sort", format!("Sort: {}", self.voice_sort.label())),
        };
        let open = self.voice_menu == Some(menu);
        let trigger_bounds = Rc::new(Cell::new(Bounds::<gpui::Pixels>::default()));
        let measured_bounds = trigger_bounds.clone();
        let labels = match menu {
            VoiceMenu::Filter => VoiceFilter::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
            VoiceMenu::Sort => VoiceSort::ALL
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>(),
        };
        let option_count = labels.len();
        let mut options = div()
            .id(format!("{id}-menu"))
            .role(Role::Menu)
            .aria_label(if menu == VoiceMenu::Filter {
                "Filter voices"
            } else {
                "Sort voices"
            })
            .track_focus(&self.voice_menu_focus)
            .w(px(210.))
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .rounded(px(7.5))
            .border_1()
            .border_color(rgb(0x808080).opacity(0.8))
            .overflow_hidden()
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.voice_menu = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "down" => {
                        this.voice_menu_cursor = (this.voice_menu_cursor + 1) % option_count;
                        window.focus(&this.voice_menu_focus, cx);
                    }
                    "up" => {
                        this.voice_menu_cursor =
                            (this.voice_menu_cursor + option_count - 1) % option_count;
                        window.focus(&this.voice_menu_focus, cx);
                    }
                    "enter" | "space" if this.voice_menu_focus.is_focused(window) => {
                        this.choose_voice_menu(menu, this.voice_menu_cursor, window, cx);
                    }
                    "escape" => {
                        this.voice_menu = None;
                        window.focus(
                            match menu {
                                VoiceMenu::Filter => &this.voice_filter_focus,
                                VoiceMenu::Sort => &this.voice_sort_focus,
                            },
                            cx,
                        );
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }));
        for (index, label) in labels.into_iter().enumerate() {
            let selected = match menu {
                VoiceMenu::Filter => self.voice_filter == VoiceFilter::ALL[index],
                VoiceMenu::Sort => self.voice_sort == VoiceSort::ALL[index],
            };
            options = options.child(
                AppButton::menu(
                    (id, index).into(),
                    div()
                        .id((id, index))
                        .role(Role::Button)
                        .focusable()
                        .tab_stop(true)
                        .aria_label(label)
                        .child(label)
                        .cursor_pointer()
                        .px(px(11.25))
                        .py(px(5.625))
                        .text_size(px(13.125))
                        .line_height(px(18.75))
                        .text_color(if selected { rgb(ACCENT) } else { rgb(TEXT) })
                        .when(selected, |view| {
                            view.bg(rgb(ACCENT).opacity(0.2))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        }),
                    selected,
                )
                .role(Role::MenuItemRadio)
                .aria_toggled(if selected {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .when(self.voice_menu_cursor == index, |view| {
                    view.aria_active_descendant()
                        .when(!selected, |view| view.bg(rgb(0x808080).opacity(0.1)))
                })
                .w_full()
                .border_0()
                .flex()
                .items_center()
                .justify_between()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_voice_menu(menu, index, window, cx)
                })),
            );
        }
        tool_button(
            id,
            label,
            match menu {
                VoiceMenu::Filter => self.voice_filter != VoiceFilter::All,
                VoiceMenu::Sort => self.voice_sort != VoiceSort::Name,
            },
        )
        .relative()
        .track_focus(match menu {
            VoiceMenu::Filter => &self.voice_filter_focus,
            VoiceMenu::Sort => &self.voice_sort_focus,
        })
        .h(px(30.))
        .py_0()
        .flex()
        .items_center()
        .gap(px(5.625))
        .aria_label(if menu == VoiceMenu::Filter {
            format!("Filter: {}", self.voice_filter.label())
        } else {
            format!("Sort: {}", self.voice_sort.label())
        })
        .aria_expanded(open)
        .child(motion::tween(
            (
                "voice-chevron",
                if menu == VoiceMenu::Filter {
                    0usize
                } else {
                    1usize
                },
            ),
            Icon::ChevronDown
                .element(13.125)
                .text_color(rgb(TEXT).opacity(0.6)),
            if open { std::f32::consts::PI } else { 0. },
            200,
            false,
            |icon, angle| {
                icon.with_transformation(gpui::Transformation::rotate(gpui::radians(angle)))
            },
        ))
        .child(
            canvas(
                move |bounds, _, _| measured_bounds.set(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            if open {
                this.voice_menu = None;
                window.focus(
                    match menu {
                        VoiceMenu::Filter => &this.voice_filter_focus,
                        VoiceMenu::Sort => &this.voice_sort_focus,
                    },
                    cx,
                );
                cx.notify();
                return;
            }
            this.voice_menu = Some(menu);
            let bounds = trigger_bounds.get();
            this.voice_menu_anchor = match menu {
                VoiceMenu::Filter => bounds.bottom_left(),
                VoiceMenu::Sort => bounds.bottom_right(),
            } + point(px(0.), px(4.));
            this.voice_menu_cursor = match menu {
                VoiceMenu::Filter => VoiceFilter::ALL
                    .iter()
                    .position(|value| *value == this.voice_filter)
                    .unwrap(),
                VoiceMenu::Sort => VoiceSort::ALL
                    .iter()
                    .position(|value| *value == this.voice_sort)
                    .unwrap(),
            };
            window.focus(&this.voice_menu_focus, cx);
            cx.notify();
        }))
        .when(open, |view| {
            view.child(
                deferred(
                    anchored()
                        .anchor(if menu == VoiceMenu::Filter {
                            Anchor::TopLeft
                        } else {
                            Anchor::TopRight
                        })
                        .position(self.voice_menu_anchor)
                        .snap_to_window_with_margin(px(8.))
                        .child(options),
                )
                .priority(1),
            )
        })
    }

    pub(super) fn voices(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let mut ids = voices::visible_voices(
            self.voice_filter,
            self.installed_only,
            self.voice_sort,
            &voices::RATINGS.voices,
            |id| self.reader.assets_for_voice(id).ready(),
        );
        let query = self.voice_query.to_lowercase();
        ids.retain(|id| {
            let voice = &VOICES[*id as usize];
            format!("{} {} {}", voice.name, voice.info, voice.source_id)
                .to_lowercase()
                .contains(&query)
        });
        let host = view_pointer(window).map(|pointer| pointer as usize);
        let search = div()
            .id("voice-search-surface")
            .key_context("NativeVoiceSearch")
            .track_focus(&self.search_focus)
            .tab_stop(true)
            .flex_shrink_0()
            .h(px(35.75))
            .rounded_lg()
            .border_1()
            .border_color(if self.search_focused {
                rgb(ACCENT)
            } else {
                rgb(0x808080).opacity(0.4)
            })
            .bg(rgb(0x808080).opacity(0.1))
            .flex()
            .items_center()
            .px_3()
            .gap_2()
            .child(Icon::Search.element(15.).text_color(rgb(TEXT).opacity(0.4)))
            .child(
                canvas(
                    move |bounds, window, _| {
                        if let Some(host) = host {
                            let clip = window.content_mask().bounds;
                            unsafe {
                                native::search_layout(
                                    host as *mut c_void,
                                    [
                                        f32::from(bounds.left()) as f64,
                                        f32::from(bounds.top()) as f64,
                                        f32::from(bounds.size.width) as f64,
                                        f32::from(bounds.size.height) as f64,
                                    ],
                                    [
                                        f32::from(clip.left()) as f64,
                                        f32::from(clip.top()) as f64,
                                        f32::from(clip.size.width) as f64,
                                        f32::from(clip.size.height) as f64,
                                    ],
                                )
                            };
                        }
                    },
                    |_, _, _, _| {},
                )
                .flex_1()
                .h(px(18.75)),
            );
        let toolbar = div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.125))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(TEXT).opacity(0.6))
                    .child(format!("{} voices", ids.len())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.voice_menu_button(VoiceMenu::Filter, cx))
                    .child(
                        tool_button("installed-voices", "Installed", self.installed_only)
                            .aria_toggled(if self.installed_only {
                                gpui::Toggled::True
                            } else {
                                gpui::Toggled::False
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.installed_only = !this.installed_only;
                                cx.notify();
                            })),
                    )
                    .child(self.voice_menu_button(VoiceMenu::Sort, cx)),
            );
        let mut cards = div()
            .id("voice-list")
            .role(Role::RadioGroup)
            .aria_label("English voices")
            .flex_shrink_0()
            .mx(px(-3.))
            .px(px(3.))
            .flex()
            .flex_col()
            .gap_3();
        if self.voice_filter == VoiceFilter::ReferenceStyle {
            cards = cards.child(div().text_size(px(12.)).text_color(rgb(TEXT).opacity(0.6)).child("Style follows the reference voice. CosyVoice and Sopro couple style to identity."));
        }
        if ids.is_empty() {
            cards =
                cards.child(
                    div()
                        .p_4()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .child(if self.voice_filter == VoiceFilter::JointDialogue {
                            "No models support joint dialogue"
                        } else {
                            "No voices match"
                        })
                        .child(button("clear-voice-filters", "Clear filters").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.voice_filter = VoiceFilter::All;
                                this.installed_only = false;
                                this.voice_query.clear();
                                native::clear_search();
                                cx.notify();
                            }),
                        )),
                );
        }
        for voice in ids {
            cards = cards.child(self.voice_card(voice, cx));
        }
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(18.75))
                    .line_height(px(26.25))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Voices"),
            )
            .child(search)
            .child(toolbar)
            .child(cards)
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.25))
                    .line_height(px(15.))
                    .text_color(rgb(TEXT).opacity(0.6))
                    .child("Startup speed · M4 benchmark, 1×"),
            )
    }

    fn voice_card(&self, voice: u32, cx: &mut Context<Self>) -> card::AppCard {
        let index = voice as usize;
        let descriptor = &VOICES[index];
        let selected = self.settings.speech.voice == voice;
        let paths = self.reader.assets_for_voice(voice);
        let installed = paths.ready();
        let progress = self
            .downloading
            .as_ref()
            .filter(|progress| progress.model == paths.model);
        let fraction = progress.map(|progress| {
            (progress.done as f64 / progress.total.max(1) as f64).clamp(0., 1.) as f32
        });
        let download_mb = assets::manifest(paths.model)
            .iter()
            .map(|asset| asset.bytes)
            .sum::<u64>() as f64
            / 1_000_000.;
        let action = if progress.is_some() {
            "Cancel".into()
        } else if installed {
            "Preview".into()
        } else if paths.model.local_key().is_some() {
            "Set up".into()
        } else {
            format!("Download · {download_mb:.0} MB")
        };
        let action_label = if progress.is_some() {
            format!("Cancel {} download", descriptor.name)
        } else if installed {
            format!("Preview {}", descriptor.name)
        } else if paths.model.local_key().is_some() {
            format!("Set up {}", descriptor.name)
        } else {
            format!("Download {} · {download_mb:.0} MB", descriptor.name)
        };
        let rating = voices::rating(voice);
        let startup = rating.and_then(|rating| rating.startup_p95_ms);
        let quality = rating.and_then(|rating| rating.naturalness_median);
        let speed_value = startup
            .map(|ms| {
                if ms < 1000. {
                    format!("{ms:.0} ms")
                } else {
                    format!("{:.2} s", ms / 1000.)
                }
            })
            .unwrap_or("—".into());
        let quality_value = quality
            .map(|score| format!("{score:.1}"))
            .unwrap_or("Unrated".into());
        let quality_detail = quality
            .map(|score| {
                format!(
                    "Predicted naturalness: {score:.2}/5 (UTMOS). {}",
                    voices::RATINGS.quality_detail
                )
            })
            .unwrap_or("Naturalness has not been evaluated for this voice.".into());
        let speed_detail = startup
            .map(|ms| format!("Startup p95: {ms:.1} ms. {}", voices::RATINGS.detail))
            .unwrap_or("Startup speed has not been measured for this voice.".into());
        let on_identity = cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            if !selected {
                this.settings.speech.voice = voice;
                this.preferences_changed();
                cx.notify();
            }
        });
        let on_card = cx.listener(move |this, _, _, cx| {
            if !selected {
                this.settings.speech.voice = voice;
                this.preferences_changed();
                cx.notify();
            }
        });
        let on_preview = cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            if !this.reader.assets_for_voice(voice).ready()
                || this
                    .downloading
                    .as_ref()
                    .is_some_and(|p| p.model == VOICES[index].model)
            {
                this.download_voice(voice, cx);
            } else {
                this.preview_voice(voice, cx);
            }
            cx.notify();
        });
        let card_id: ElementId = ("voice-card", index).into();
        card::AppCard::new(card_id.clone(), selected, move |scale, paint| {
            let p = |value: f32| px(value * scale);
            let heading = div()
                .flex()
                .items_center()
                .gap(p(11.25))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(p(15.))
                        .line_height(p(22.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(descriptor.name),
                )
                .when(selected, |view| {
                    view.child(
                        div()
                            .flex()
                            .items_center()
                            .rounded_full()
                            .px(p(11.25))
                            .py(p(3.75))
                            .bg(rgb(ACCENT))
                            .text_color(rgb(TEXT))
                            .text_size(p(11.25))
                            .line_height(p(15.))
                            .child(Icon::Check.element(11.25 * scale).text_color(rgb(TEXT)))
                            .child(div().ml(p(3.75)).child("Selected")),
                    )
                });
            let identity = div()
                .id(("voice", index))
                .role(Role::RadioButton)
                .focusable()
                .tab_stop(true)
                .aria_label(descriptor.name)
                .aria_description(descriptor.info)
                .aria_toggled(if selected {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .focus_visible(|style| style.rounded_sm().border_1().border_color(rgb(ACCENT)))
                .child(heading)
                .child(
                    div()
                        .text_size(p(13.125))
                        .line_height(p(21.328125))
                        .text_color(rgb(TEXT).opacity(0.6))
                        .child(descriptor.info),
                )
                .on_click(on_identity);
            let ratings = div()
                .ml(p(15.))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(p(3.75))
                .child(indicator(
                    ("voice-quality", index),
                    if quality.is_some() {
                        "Predicted"
                    } else {
                        "Quality"
                    },
                    quality_value,
                    quality.map(|score| ((score - 1.) / 4.).clamp(0., 1.)),
                    quality_detail,
                    scale,
                ))
                .child(indicator(
                    ("voice-startup", index),
                    "Speed",
                    speed_value,
                    startup.map(|ms| (1. - ms / 1000.).clamp(0., 1.) as f32),
                    speed_detail,
                    scale,
                ));
            let action_id: ElementId = ("preview", index).into();
            let action_button = AppButton::accent_ghost(
                action_id.clone(),
                div()
                    .id(action_id)
                    .role(Role::Button)
                    .focusable()
                    .tab_stop(true)
                    .aria_label(action_label)
                    .cursor_pointer()
                    .rounded_lg()
                    .border(p(1.))
                    .border_color(rgb(ACCENT).opacity(0.))
                    .px(p(7.5))
                    .py(p(3.75))
                    .flex()
                    .items_center()
                    .gap(p(5.625))
                    .text_size(p(11.25))
                    .line_height(p(15.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(ACCENT).opacity(0.85))
                    .when(fraction.is_none(), |view| {
                        view.child(
                            if installed {
                                Icon::Play
                            } else {
                                Icon::Download
                            }
                            .inherited(13.125 * scale),
                        )
                    })
                    .child(action)
                    .on_click(on_preview),
            );
            let model_name = if descriptor.model == SpeechModel::KokoroHeart {
                "Kokoro"
            } else {
                "Kitten nano"
            };
            let (footer_action, progress_action) = if fraction.is_some() {
                (None, Some(action_button))
            } else {
                (Some(action_button), None)
            };
            let progress_row = fraction.map(|fraction| {
                div()
                    .w_full()
                    .mt(p(11.25))
                    .child(DownloadMeter {
                        id: index,
                        fraction,
                        scale,
                    })
                    .child(
                        div()
                            .mt(p(3.75))
                            .h(p(22.5))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(p(11.25))
                            .line_height(p(15.))
                            .text_color(rgb(TEXT).opacity(0.5))
                            .child(if fraction >= 1. {
                                "Preparing voices…".into()
                            } else {
                                format!("Downloading · {:.0}%", fraction * 100.)
                            })
                            .children(progress_action),
                    )
            });
            div()
                .id(card_id.clone())
                .role(Role::Group)
                .aria_label(format!("{} voice", descriptor.name))
                .flex_shrink_0()
                .px(p(15.))
                .py(p(11.25))
                .rounded(p(11.25))
                .border(p(2.))
                .border_color(paint.border)
                .bg(paint.background)
                .text_color(paint.foreground)
                .flex()
                .flex_col()
                .gap(p(7.5))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(identity)
                        .child(ratings),
                )
                .child(div().h(p(1.)).bg(rgb(0x808080).opacity(0.2)))
                .child(
                    div()
                        .h(p(18.75))
                        .mt(p(1.875))
                        .mb(p(-1.875))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(p(11.25))
                                .line_height(p(15.))
                                .text_color(rgb(TEXT).opacity(0.5))
                                .child(model_name),
                        )
                        .children(footer_action),
                )
                .children(progress_row)
                .on_click(on_card)
        })
        .height(card::HEIGHT + if fraction.is_some() { 50.625 } else { 0. })
    }
}
