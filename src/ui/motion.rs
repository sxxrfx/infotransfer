use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Rgba, Window,
};
use std::time::{Duration, Instant};

/// Tailwind v4's default timing function.
pub fn standard_ease(x: f32) -> f32 {
    cubic_bezier(x, [0.4, 0., 0.2, 1.])
}
pub fn cubic_bezier(x: f32, curve: [f32; 4]) -> f32 {
    if x <= 0. {
        return 0.;
    }
    if x >= 1. {
        return 1.;
    }
    let sample = |t: f32, a: f32, b: f32| {
        3. * (1. - t).powi(2) * t * a + 3. * (1. - t) * t * t * b + t.powi(3)
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let t = (low + high) * 0.5;
        if sample(t, curve[0], curve[2]) < x {
            low = t;
        } else {
            high = t;
        }
    }
    sample((low + high) * 0.5, curve[1], curve[3])
}
#[derive(Clone, Copy)]
pub enum Curve {
    Standard,
    Linear,
    Compact,
}
impl Curve {
    fn apply(self, value: f32) -> f32 {
        match self {
            Self::Standard => standard_ease(value),
            Self::Linear => value,
            Self::Compact => cubic_bezier(value, [0.22, 1., 0.36, 1.]),
        }
    }
}

pub trait TweenValue: Copy + PartialEq + 'static {
    fn between(self, target: Self, progress: f32) -> Self;
}
impl TweenValue for f32 {
    fn between(self, target: Self, progress: f32) -> Self {
        self + (target - self) * progress
    }
}
impl TweenValue for Hsla {
    fn between(self, target: Self, progress: f32) -> Self {
        let a = Rgba::from(self);
        let b = Rgba::from(target);
        Rgba {
            r: a.r.between(b.r, progress),
            g: a.g.between(b.g, progress),
            b: a.b.between(b.b, progress),
            a: a.a.between(b.a, progress),
        }
        .into()
    }
}
#[derive(Clone, Copy, PartialEq)]
pub struct Paint {
    pub background: Hsla,
    pub border: Hsla,
    pub foreground: Hsla,
}
impl TweenValue for Paint {
    fn between(self, target: Self, progress: f32) -> Self {
        Self {
            background: self.background.between(target.background, progress),
            border: self.border.between(target.border, progress),
            foreground: self.foreground.between(target.foreground, progress),
        }
    }
}

struct TweenState<V> {
    from: V,
    target: V,
    started: Instant,
}
impl<V: TweenValue> TweenState<V> {
    fn sample(&self, now: Instant, duration: Duration, curve: Curve) -> (V, bool) {
        let elapsed = now.duration_since(self.started).as_secs_f32();
        let progress = (elapsed / duration.as_secs_f32()).min(1.);
        let progress = curve.apply(progress);
        (
            self.from.between(self.target, progress),
            elapsed < duration.as_secs_f32() && self.from != self.target,
        )
    }
}

/// Retains the current value when a target changes, including reversed hovers.
pub struct Tween<E, V> {
    id: ElementId,
    element: Option<E>,
    target: V,
    duration: Duration,
    curve: Curve,
    apply: Box<dyn FnOnce(E, V) -> E>,
}
pub fn tween<E: IntoElement + 'static, V: TweenValue>(
    id: impl Into<ElementId>,
    element: E,
    target: V,
    milliseconds: u64,
    linear: bool,
    apply: impl FnOnce(E, V) -> E + 'static,
) -> Tween<E, V> {
    Tween {
        id: id.into(),
        element: Some(element),
        target,
        duration: Duration::from_millis(milliseconds),
        curve: if linear {
            Curve::Linear
        } else {
            Curve::Standard
        },
        apply: Box::new(apply),
    }
}
impl<E, V> Tween<E, V> {
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }
}
impl<E: IntoElement + 'static, V: TweenValue> IntoElement for Tween<E, V> {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}
impl<E: IntoElement + 'static, V: TweenValue> Element for Tween<E, V> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        window.with_element_state(
            id.expect("transition id"),
            |state: Option<TweenState<V>>, window| {
                let now = Instant::now();
                let mut state = state.unwrap_or(TweenState {
                    from: self.target,
                    target: self.target,
                    started: now,
                });
                if state.target != self.target {
                    state.from = state.sample(now, self.duration, self.curve).0;
                    state.target = self.target;
                    state.started = now;
                }
                let (value, running) = if cx.reduce_motion() {
                    (self.target, false)
                } else {
                    state.sample(now, self.duration, self.curve)
                };
                if running {
                    window.request_animation_frame();
                }
                let apply = std::mem::replace(&mut self.apply, Box::new(|element, _| element));
                let mut element = apply(self.element.take().expect("transition element"), value)
                    .into_any_element();
                ((element.request_layout(window, cx), element), state)
            },
        )
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}
