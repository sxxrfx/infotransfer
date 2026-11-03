use super::motion::{Paint, TweenValue, tween};
use super::*;
use gpui::{Hsla, RenderOnce};

pub(super) const HEIGHT: f32 = 105.078125;
type Builder = Box<dyn FnOnce(f32, Paint) -> Stateful<Div>>;

#[derive(Default)]
struct Interaction {
    hovered: bool,
    pressed: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct Visual {
    scale: f32,
    paint: Paint,
    shadow: f32,
}
impl TweenValue for Visual {
    fn between(self, target: Self, progress: f32) -> Self {
        Self {
            scale: self.scale.between(target.scale, progress),
            paint: self.paint.between(target.paint, progress),
            shadow: self.shadow.between(target.shadow, progress),
        }
    }
}

#[derive(IntoElement)]
pub(super) struct AppCard {
    id: ElementId,
    selected: bool,
    height: f32,
    build: Builder,
}
impl AppCard {
    pub fn new(
        id: ElementId,
        selected: bool,
        build: impl FnOnce(f32, Paint) -> Stateful<Div> + 'static,
    ) -> Self {
        Self {
            id,
            selected,
            height: HEIGHT,
            build: Box::new(build),
        }
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}
#[derive(IntoElement)]
struct CardFrame {
    build: Builder,
    visual: Visual,
    height: f32,
}
impl RenderOnce for CardFrame {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let scale = self.visual.scale;
        let shadow = self.visual.shadow;
        // Reserve the resting layout box; scale the component's own metrics so
        // hover/press never moves adjacent cards. GPUI has no Div transform API.
        div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_shrink_0()
            .child(
                (self.build)(scale, self.visual.paint)
                    .when(shadow > 0., |view| {
                        view.shadow(vec![
                            gpui::BoxShadow {
                                color: rgb(0).opacity(shadow).into(),
                                offset: point(px(0.), px(10. * scale)),
                                blur_radius: px(15. * scale),
                                spread_radius: px(-3. * scale),
                                inset: false,
                            },
                            gpui::BoxShadow {
                                color: rgb(0).opacity(shadow).into(),
                                offset: point(px(0.), px(4. * scale)),
                                blur_radius: px(6. * scale),
                                spread_radius: px(-4. * scale),
                                inset: false,
                            },
                        ])
                    })
                    .absolute()
                    .left(gpui::relative((1. - scale) * 0.5))
                    .top(px(self.height * (1. - scale) * 0.5))
                    .w(gpui::relative(scale))
                    .h(px(self.height * scale)),
            )
    }
}
impl RenderOnce for AppCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.with_element_namespace(self.id.clone(), |window| {
            window.use_keyed_state("card-interaction", cx, |_, _| Interaction::default())
        });
        let hovered = state.read(cx).hovered && !self.selected;
        let pressed = state.read(cx).pressed && !self.selected;
        let neutral: Hsla = rgb(0x808080).opacity(0.2).into();
        let target = Visual {
            shadow: if hovered { 0.1 } else { 0. },
            scale: if pressed {
                0.99
            } else if hovered {
                1.01
            } else {
                1.
            },
            paint: Paint {
                background: rgb(ACCENT)
                    .opacity(if self.selected {
                        0.1
                    } else if hovered {
                        0.05
                    } else {
                        0.
                    })
                    .into(),
                border: if self.selected || hovered {
                    rgb(ACCENT).opacity(0.5).into()
                } else {
                    neutral
                },
                foreground: rgb(if hovered { ACCENT } else { TEXT }).into(),
            },
        };
        let hover_state = state.clone();
        let down_state = state.clone();
        let up_state = state.clone();
        let build = self.build;
        let frame = CardFrame {
            visual: target,
            height: self.height,
            build: Box::new(move |scale, paint| {
                build(scale, paint)
                    .on_hover(move |hovered, _, cx| {
                        hover_state.update(cx, |state, cx| {
                            state.hovered = *hovered;
                            if !*hovered {
                                state.pressed = false;
                            }
                            cx.notify();
                        })
                    })
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        down_state.update(cx, |state, cx| {
                            state.pressed = true;
                            cx.notify();
                        })
                    })
                    .on_mouse_up(gpui::MouseButton::Left, move |_, _, cx| {
                        up_state.update(cx, |state, cx| {
                            state.pressed = false;
                            cx.notify();
                        })
                    })
                    .on_mouse_up_out(gpui::MouseButton::Left, move |_, _, cx| {
                        state.update(cx, |state, cx| {
                            state.pressed = false;
                            cx.notify();
                        })
                    })
            }),
        };
        tween(self.id, frame, target, 200, false, |mut frame, visual| {
            frame.visual = visual;
            frame
        })
    }
}
