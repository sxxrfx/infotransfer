use super::*;
use infotransfer::activation::{ActivationKey, KeyPreference};

pub(super) fn panel(
    activation_key: ActivationKey,
    preference: KeyPreference,
    on_change: impl Fn(&KeyPreference, &mut Window, &mut App) + 'static,
) -> Div {
    let keycap = |id: &'static str, label: &'static str, accessible_label: &'static str| {
        div()
            .id(id)
            .role(Role::Label)
            .aria_label(accessible_label)
            .min_w(px(40.))
            .h(px(30.))
            .px_3()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x808080).opacity(0.1))
            .text_size(px(13.125))
            .font_weight(gpui::FontWeight::MEDIUM)
            .line_height(px(18.75))
            .child(label)
    };
    let key_label = match activation_key {
        ActivationKey::Fn => "Fn",
        ActivationKey::RightOption => "Right ⌥",
    };
    div().child(section("Shortcuts")).child(
        group()
            .child(key_row(preference, on_change))
            .child(
                row("Tap to read")
                    .id("tap-shortcut-row")
                    .role(Role::Group)
                    .aria_label("Tap to read")
                    .aria_description("Tap the key to read the current selection on release.")
                    .child(keycap(
                        "tap-shortcut",
                        key_label,
                        activation_key.accessible_label(),
                    )),
            )
            .child(
                row_content(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(setting_label("Hold to select"))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(rgb(TEXT).opacity(0.6))
                                .child("Release to read"),
                        ),
                )
                .id("hold-shortcut-row")
                .role(Role::Group)
                .aria_label("Hold to select")
                .aria_description("Hold the key, select text, then release to read.")
                .child(keycap(
                    "hold-shortcut",
                    key_label,
                    activation_key.accessible_label(),
                )),
            )
            .child(
                row("Cancel audio")
                    .id("cancel-shortcut-row")
                    .role(Role::Group)
                    .aria_label("Cancel audio, Escape")
                    .border_b_0()
                    .child(keycap("cancel-shortcut", "Esc", "Escape")),
            ),
    )
}

fn key_row(
    current: KeyPreference,
    on_change: impl Fn(&KeyPreference, &mut Window, &mut App) + 'static,
) -> Div {
    let on_change = std::rc::Rc::new(on_change);
    let mut choices = div().id("activation-key").flex().gap_1();
    for preference in infotransfer::activation::KeyPreference::ALL {
        let selected = current == preference;
        choices = choices.child(
            tool_button(preference.label(), preference.label(), selected)
                .role(Role::RadioButton)
                .aria_label(match preference {
                    infotransfer::activation::KeyPreference::Auto => "Automatic activation key",
                    infotransfer::activation::KeyPreference::Fn => "Fn / Globe",
                    infotransfer::activation::KeyPreference::RightOption => "Right Option",
                })
                .aria_toggled(if selected {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .on_key_down({
                    let on_change = on_change.clone();
                    move |event, window, cx| {
                        let next = match event.keystroke.key.as_str() {
                            "enter" | "space" => Some(preference),
                            "left" | "up" => Some(match current {
                                KeyPreference::Auto => KeyPreference::RightOption,
                                KeyPreference::Fn => KeyPreference::Auto,
                                KeyPreference::RightOption => KeyPreference::Fn,
                            }),
                            "right" | "down" => Some(match current {
                                KeyPreference::Auto => KeyPreference::Fn,
                                KeyPreference::Fn => KeyPreference::RightOption,
                                KeyPreference::RightOption => KeyPreference::Auto,
                            }),
                            _ => None,
                        };
                        if let Some(next) = next {
                            on_change(&next, window, cx);
                            cx.stop_propagation();
                        }
                    }
                })
                .on_click({
                    let on_change = on_change.clone();
                    move |_, window, cx| on_change(&preference, window, cx)
                }),
        );
    }
    row("Read key").child(choices.role(Role::RadioGroup).aria_label("Activation key"))
}
