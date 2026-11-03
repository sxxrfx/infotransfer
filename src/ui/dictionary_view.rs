use super::*;

impl Desktop {
    pub(super) fn dictionary(&self, cx: &mut Context<Self>) -> Div {
        let mut entries = group();
        for (index, entry) in self.settings.speech.dictionary.iter().enumerate() {
            let mut label = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(setting_label(&entry.word).truncate())
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(rgb(TEXT).opacity(0.6))
                        .truncate()
                        .child(entry.replacement.clone()),
                );
            if entry.match_case {
                label = label.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(rgb(TEXT).opacity(0.6))
                        .child("Case-sensitive"),
                );
            }
            entries = entries.child(
                row_content(label)
                    .id(("dictionary-entry", index))
                    .role(Role::Group)
                    .aria_label(format!("{} reads as {}", entry.word, entry.replacement))
                    .when(index + 1 == self.settings.speech.dictionary.len(), |row| {
                        row.border_b_0()
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                button(("preview-word", index), "")
                                    .aria_label(format!("Preview {}", entry.word))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::Play.element(14.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let word =
                                            this.settings.speech.dictionary[index].word.clone();
                                        this.cancel_capture();
                                        this.capture
                                            .popup_dismissed
                                            .store(false, Ordering::Release);
                                        native::player_position(0);
                                        if let Err(error) = this.reader.read(word, Origin::Preview)
                                        {
                                            this.reader.notice(error.to_string());
                                        }
                                        this.update_player(cx);
                                    })),
                            )
                            .child(
                                button(("edit-word", index), "Edit")
                                    .aria_label(format!("Edit {}", entry.word))
                                    .on_click(cx.listener(move |this, _, window, _| {
                                        if let Some(view) = view_pointer(window) {
                                            unsafe {
                                                native::edit_dictionary(
                                                    view,
                                                    Some(index),
                                                    &this.settings.speech.dictionary,
                                                )
                                            };
                                        }
                                    })),
                            )
                            .child(
                                button(("remove-word", index), "Remove")
                                    .aria_label(format!("Remove {}", entry.word))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.settings.speech.dictionary.remove(index);
                                        this.preferences_changed();
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        if self.settings.speech.dictionary.is_empty() {
            entries = entries.child(row("No word replacements").border_b_0());
        }
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section("Word replacements").mb_0())
                    .child(
                        button("add-word", "Add")
                            .aria_label("Add word replacement")
                            .on_click(cx.listener(|this, _, window, _| {
                                if let Some(view) = view_pointer(window) {
                                    unsafe {
                                        native::edit_dictionary(
                                            view,
                                            None,
                                            &this.settings.speech.dictionary,
                                        )
                                    };
                                }
                            })),
                    ),
            )
            .child(entries)
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(rgb(TEXT).opacity(0.6))
                    .child("Common contractions expand automatically."),
            )
    }
}
