//! Tap/hold classification independent of Accessibility and the UI event loop.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPreference {
    #[default]
    Auto,
    Fn,
    RightOption,
}
impl KeyPreference {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Fn, Self::RightOption];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Fn => "Fn",
            Self::RightOption => "Right ⌥",
        }
    }
    pub fn resolve(self, fn_unassigned: bool) -> ActivationKey {
        match self {
            Self::Fn => ActivationKey::Fn,
            Self::Auto if fn_unassigned => ActivationKey::Fn,
            _ => ActivationKey::RightOption,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationKey {
    Fn,
    RightOption,
}
impl ActivationKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fn => "Fn",
            Self::RightOption => "right ⌥",
        }
    }
    pub fn accessible_label(self) -> &'static str {
        match self {
            Self::Fn => "Fn / Globe",
            Self::RightOption => "Right Option",
        }
    }
    fn code(self) -> u16 {
        match self {
            Self::Fn => 63,
            Self::RightOption => 61,
        }
    }
    fn flag(self) -> u8 {
        match self {
            Self::Fn => 1,
            Self::RightOption => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEdge {
    Press,
    Release,
    Cancel,
}
// Native code reduces modifier flags to Fn=1, right Option=2, other modifiers=4.
// Key combinations cancel until release; normal typing never becomes a tap.
pub struct ModifierGesture {
    pub key: ActivationKey,
    pressed: bool,
    blocked: bool,
}
impl ModifierGesture {
    pub const fn new(key: ActivationKey) -> Self {
        Self {
            key,
            pressed: false,
            blocked: false,
        }
    }
    pub fn cancel(&mut self) {
        self.blocked |= self.pressed;
        self.pressed = false;
    }
    pub fn event(&mut self, code: u16, flags: u8, key_down: bool) -> Option<KeyEdge> {
        let selected_down = flags & self.key.flag() != 0;
        if !selected_down {
            self.blocked = false;
            return std::mem::take(&mut self.pressed).then_some(
                if !key_down && code == self.key.code() {
                    KeyEdge::Release
                } else {
                    KeyEdge::Cancel
                },
            );
        }
        if key_down || flags & !self.key.flag() != 0 {
            self.blocked = true;
            return std::mem::take(&mut self.pressed).then_some(KeyEdge::Cancel);
        }
        if !self.blocked && !self.pressed && code == self.key.code() {
            self.pressed = true;
            return Some(KeyEdge::Press);
        }
        None
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionFingerprint {
    pub digest: [u8; 32],
    pub range: Option<(i64, i64)>,
}
impl SelectionFingerprint {
    pub fn changed_from(&self, prior: &Self) -> bool {
        self.digest != prior.digest
            || matches!((self.range, prior.range), (Some(a), Some(b)) if a != b)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureKind {
    Baseline,
    Tap,
    Hold,
}
#[derive(Clone, Debug)]
pub struct CapturePlan {
    pub pid: i32,
    pub revision: u64,
    pub activation: u64,
    pub kind: CaptureKind,
    pub baseline: Option<SelectionFingerprint>,
}
#[derive(Debug)]
pub enum Effect {
    Capture(CapturePlan),
    Cancel,
}
enum Baseline {
    Pending,
    Known(SelectionFingerprint),
    Unavailable,
}
struct Gesture {
    pid: i32,
    revision: u64,
    down: u64,
    released: Option<u64>,
    baseline: Baseline,
}
#[derive(Default)]
pub struct Activation {
    gesture: Option<Gesture>,
}
pub const HOLD_DELAY_NS: u64 = 350_000_000;
impl Activation {
    pub fn active(&self) -> bool {
        self.gesture
            .as_ref()
            .is_some_and(|gesture| gesture.released.is_none())
    }
    pub fn cancel(&mut self) {
        self.gesture = None;
    }
    pub fn press(&mut self, pid: i32, time: u64, revision: u64) -> Option<Effect> {
        if self.active() {
            return None;
        }
        self.gesture = Some(Gesture {
            pid,
            revision,
            down: time,
            released: None,
            baseline: Baseline::Pending,
        });
        Some(Effect::Capture(CapturePlan {
            pid,
            revision,
            activation: time,
            kind: CaptureKind::Baseline,
            baseline: None,
        }))
    }
    pub fn release(&mut self, pid: i32, time: u64) -> Option<Effect> {
        let gesture = self.gesture.as_mut()?;
        if gesture.released.is_some() {
            return None;
        }
        if gesture.pid != pid {
            self.cancel();
            return Some(Effect::Cancel);
        }
        if time.saturating_sub(gesture.down) < HOLD_DELAY_NS {
            let gesture = self.gesture.take().unwrap();
            return Some(Effect::Capture(CapturePlan {
                pid,
                revision: gesture.revision,
                activation: time,
                kind: CaptureKind::Tap,
                baseline: None,
            }));
        }
        if matches!(gesture.baseline, Baseline::Pending) {
            gesture.released = Some(time);
            return None;
        }
        self.finish_hold(time)
    }
    pub fn baseline(
        &mut self,
        revision: u64,
        value: Option<SelectionFingerprint>,
    ) -> Option<Effect> {
        let gesture = self.gesture.as_mut()?;
        if gesture.revision != revision {
            return None;
        }
        gesture.baseline = value.map_or(Baseline::Unavailable, Baseline::Known);
        let released = gesture.released;
        released.and_then(|time| self.finish_hold(time))
    }
    fn finish_hold(&mut self, time: u64) -> Option<Effect> {
        let gesture = self.gesture.take()?;
        Some(match gesture.baseline {
            Baseline::Known(value) => Effect::Capture(CapturePlan {
                pid: gesture.pid,
                revision: gesture.revision,
                activation: time,
                kind: CaptureKind::Hold,
                baseline: Some(value),
            }),
            _ => Effect::Cancel,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_key_prefers_unassigned_fn_and_falls_back_conservatively() {
        assert_eq!(KeyPreference::Auto.resolve(true), ActivationKey::Fn);
        assert_eq!(
            KeyPreference::Auto.resolve(false),
            ActivationKey::RightOption
        );
        assert_eq!(
            KeyPreference::RightOption.resolve(true),
            ActivationKey::RightOption
        );
    }
    #[test]
    fn modifier_taps_and_holds_emit_single_press_release_edges() {
        for (key, code, flags) in [
            (ActivationKey::Fn, 63, 1),
            (ActivationKey::RightOption, 61, 2),
        ] {
            let mut gate = ModifierGesture::new(key);
            assert_eq!(gate.event(code, flags, false), Some(KeyEdge::Press));
            assert_eq!(gate.event(code, flags, false), None);
            assert_eq!(gate.event(code, 0, false), Some(KeyEdge::Release));
            assert_eq!(gate.event(code, 0, false), None);
        }
    }
    #[test]
    fn ordinary_modifier_combinations_never_read_on_release() {
        for (key, code, flags) in [
            (ActivationKey::Fn, 63, 1),
            (ActivationKey::RightOption, 61, 2),
        ] {
            let mut gate = ModifierGesture::new(key);
            // Fn+arrow and Option+letter cancel, including key repeat.
            assert_eq!(gate.event(code, flags, false), Some(KeyEdge::Press));
            assert_eq!(gate.event(123, flags, true), Some(KeyEdge::Cancel));
            assert_eq!(gate.event(123, flags, true), None);
            assert_eq!(gate.event(code, 0, false), None);
            // Holding another modifier before pressing the key never arms it.
            assert_eq!(gate.event(code, flags | 4, false), None);
            assert_eq!(gate.event(56, flags, false), None);
            assert_eq!(gate.event(code, 0, false), None);
            // Adding another modifier after the key also cancels.
            assert_eq!(gate.event(code, flags, false), Some(KeyEdge::Press));
            assert_eq!(gate.event(56, flags | 4, false), Some(KeyEdge::Cancel));
            assert_eq!(gate.event(code, 0, false), None);
            assert_eq!(gate.event(code, flags, false), Some(KeyEdge::Press));
        }
    }
    #[test]
    fn left_option_is_not_right_option_and_cancel_waits_for_release() {
        let mut gate = ModifierGesture::new(ActivationKey::RightOption);
        assert_eq!(gate.event(58, 4, false), None);
        assert_eq!(gate.event(58, 0, false), None);
        assert_eq!(gate.event(61, 2, false), Some(KeyEdge::Press));
        gate.cancel();
        assert_eq!(gate.event(61, 2, false), None);
        assert_eq!(gate.event(61, 0, false), None);
        assert_eq!(gate.event(61, 2, false), Some(KeyEdge::Press));
        // A missed release cannot commit a reading on a later typing event.
        assert_eq!(gate.event(0, 0, true), Some(KeyEdge::Cancel));
    }
    fn fingerprint() -> SelectionFingerprint {
        SelectionFingerprint {
            digest: [1; 32],
            range: Some((0, 4)),
        }
    }
    #[test]
    fn a_new_press_supersedes_a_released_gesture_waiting_for_capture() {
        let mut a = Activation::default();
        a.press(10, 1, 7);
        assert!(a.release(10, 500_000_000).is_none());
        assert!(a.press(10, 600_000_000, 8).is_some());
        assert!(a.baseline(7, Some(fingerprint())).is_none());
        assert!(matches!(
            a.release(10, 650_000_000),
            Some(Effect::Capture(CapturePlan {
                revision: 8,
                kind: CaptureKind::Tap,
                ..
            }))
        ));
    }
    #[test]
    fn range_unavailability_is_not_a_new_selection() {
        let prior = fingerprint();
        let mut next = prior.clone();
        next.range = None;
        assert!(!next.changed_from(&prior));
        next.range = Some((10, 4));
        assert!(next.changed_from(&prior));
        next.range = None;
        next.digest = [2; 32];
        assert!(next.changed_from(&prior));
    }
    #[test]
    fn tap_reads_on_release_and_ignores_late_baseline() {
        let mut a = Activation::default();
        a.press(10, 1, 7);
        assert!(matches!(
            a.release(10, 100_000_000),
            Some(Effect::Capture(CapturePlan {
                kind: CaptureKind::Tap,
                ..
            }))
        ));
        assert!(a.baseline(7, Some(fingerprint())).is_none());
        assert!(a.release(10, 150_000_000).is_none());
    }
    #[test]
    fn hold_waits_for_baseline_and_preserves_release_time() {
        let mut a = Activation::default();
        a.press(10, 1, 7);
        assert!(a.release(10, 500_000_000).is_none());
        let Some(Effect::Capture(plan)) = a.baseline(7, Some(fingerprint())) else {
            panic!("missing hold capture")
        };
        assert_eq!(plan.kind, CaptureKind::Hold);
        assert_eq!(plan.activation, 500_000_000);
        assert_eq!(plan.baseline, Some(fingerprint()));
    }
    #[test]
    fn repeats_focus_changes_unavailable_baselines_and_cancel_are_safe() {
        let mut a = Activation::default();
        a.press(10, 1, 7);
        assert!(a.press(10, 100, 8).is_none());
        assert!(a.baseline(8, Some(fingerprint())).is_none());
        a.baseline(7, None);
        assert!(matches!(a.release(10, 500_000_000), Some(Effect::Cancel)));
        a.press(10, 1, 9);
        assert!(matches!(a.release(11, 100), Some(Effect::Cancel)));
        a.press(10, 1, 10);
        a.cancel();
        assert!(a.release(10, 100).is_none());
    }
}
