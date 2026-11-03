use super::motion::{Paint, tween};
use super::*;
use gpui::{
    AnyElement, InteractiveElement, Interactivity, ParentElement, RenderOnce,
    StatefulInteractiveElement, StyleRefinement, Styled,
};

#[derive(Default)]
struct HoverState {
    hovered: bool,
}
enum Kind {
    Secondary,
    Ghost,
    Sidebar(bool),
    Toolbar(bool),
    Menu(bool),
    AccentGhost,
}

#[derive(IntoElement)]
pub(super) struct HandyButton {
    id: ElementId,
    inner: Stateful<Div>,
    kind: Kind,
}
impl HandyButton {
    pub fn new(id: ElementId, inner: Stateful<Div>, ghost: bool) -> Self {
        Self {
            id,
            inner,
            kind: if ghost { Kind::Ghost } else { Kind::Secondary },
        }
    }
    pub fn sidebar(id: ElementId, inner: Stateful<Div>, active: bool) -> Self {
        Self {
            id,
            inner,
            kind: Kind::Sidebar(active),
        }
    }
    pub fn toolbar(id: ElementId, inner: Stateful<Div>, active: bool) -> Self {
        Self {
            id,
            inner,
            kind: Kind::Toolbar(active),
        }
    }
    pub fn menu(id: ElementId, inner: Stateful<Div>, selected: bool) -> Self {
        Self {
            id,
            inner,
            kind: Kind::Menu(selected),
        }
    }
    pub fn accent_ghost(id: ElementId, inner: Stateful<Div>) -> Self {
        Self {
            id,
            inner,
            kind: Kind::AccentGhost,
        }
    }
}
impl Styled for HandyButton {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}
impl InteractiveElement for HandyButton {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}
impl StatefulInteractiveElement for HandyButton {}
impl ParentElement for HandyButton {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}
impl RenderOnce for HandyButton {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.with_element_namespace(self.id.clone(), |window| {
            window.use_keyed_state("handy-hover", cx, |_, _| HoverState::default())
        });
        let hovered = state.read(cx).hovered;
        let background = self
            .inner
            .style()
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|fill| fill.as_solid())
            .unwrap_or_else(gpui::transparent_black);
        let border = self
            .inner
            .style()
            .border_color
            .unwrap_or_else(gpui::transparent_black);
        let foreground = self
            .inner
            .style()
            .text
            .color
            .unwrap_or_else(|| rgb(TEXT).into());
        let (hover_background, hover_border, hover_foreground) = match self.kind {
            Kind::Secondary => (
                rgb(0xda5893).opacity(0.3).into(),
                rgb(PINK).into(),
                foreground,
            ),
            Kind::Ghost => (
                rgb(0x808080).opacity(0.1).into(),
                rgb(PINK).into(),
                foreground,
            ),
            Kind::Sidebar(true) => (background, border, foreground),
            Kind::Sidebar(false) | Kind::Toolbar(false) => {
                (rgb(0x808080).opacity(0.2).into(), border, foreground)
            }
            Kind::Toolbar(true) => (rgb(PINK).opacity(0.3).into(), border, foreground),
            Kind::Menu(true) => (background, border, foreground),
            Kind::Menu(false) => (rgb(0x808080).opacity(0.1).into(), border, foreground),
            Kind::AccentGhost => (
                rgb(PINK).opacity(0.1).into(),
                rgb(PINK).into(),
                rgb(PINK).into(),
            ),
        };
        let target = Paint {
            background: if hovered {
                hover_background
            } else {
                background
            },
            border: if hovered { hover_border } else { border },
            foreground: if hovered {
                hover_foreground
            } else {
                foreground
            },
        };
        let inner = self.inner.on_hover(move |hovered, _, cx| {
            state.update(cx, |state, cx| {
                if state.hovered != *hovered {
                    state.hovered = *hovered;
                    cx.notify();
                }
            })
        });
        tween(self.id, inner, target, 150, false, |view, paint| {
            view.bg(paint.background)
                .border_color(paint.border)
                .text_color(paint.foreground)
        })
    }
}
