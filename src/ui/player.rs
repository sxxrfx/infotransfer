use super::*;
use gpui::{Animation, AnimationExt};
use std::collections::VecDeque;

pub(super) struct Player {
    pub reader: Reader,
    pub capture: Arc<CaptureState>,
    bars: VecDeque<f32>,
    phase: Phase,
    capturing: bool,
    visible: bool,
    escape: bool,
    notice: Option<String>,
    last_run: u64,
    smoothed: f32,
}
impl Player {
    pub fn new(reader: Reader, capture: Arc<CaptureState>) -> Self {
        Self {
            reader,
            capture,
            bars: VecDeque::from(vec![0.; 9]),
            phase: Phase::Idle,
            capturing: false,
            visible: false,
            escape: false,
            notice: None,
            last_run: 0,
            smoothed: 0.,
        }
    }
    pub fn synchronize(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.reader.snapshot();
        let run = snapshot.run.as_ref().map_or(0, |run| run.id);
        if run != self.last_run {
            for bar in &mut self.bars {
                *bar = 0.;
            }
            self.last_run = run;
            self.smoothed = 0.;
        }
        let capturing = self.capture.active.load(Ordering::Acquire);
        let visible = (capturing || snapshot.phase != Phase::Idle || snapshot.notice.is_some())
            && !self.capture.popup_dismissed.load(Ordering::Acquire);
        if visible != self.visible {
            native::player_visible(visible);
            self.visible = visible;
        }
        // Escape is taken from other apps only while something is being read. A notice
        // or a paused reading (for example after the Mac wakes) leaves it alone.
        let escape =
            visible && (capturing || matches!(snapshot.phase, Phase::Preparing | Phase::Reading));
        if escape != self.escape {
            if !native::escape_active(escape) {
                self.reader
                    .notice("Escape shortcut unavailable. Use Stop in the menu bar.".into());
            }
            self.escape = escape;
        }
        let changed = self.phase != snapshot.phase
            || self.capturing != capturing
            || self.notice != snapshot.notice;
        self.phase = snapshot.phase;
        self.capturing = capturing;
        self.notice = snapshot.notice;
        if visible && self.phase == Phase::Reading && !native::reduced_motion() {
            self.smoothed = self.smoothed * 0.7 + (self.reader.level() * 4.).clamp(0., 1.) * 0.3;
            self.bars.pop_front();
            self.bars.push_back(self.smoothed);
            cx.notify();
        } else if changed {
            cx.notify();
        }
    }
}
impl Render for Player {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        visual(
            self.phase,
            self.capturing,
            self.notice.as_deref(),
            &self.bars,
            native::reduced_motion(),
        )
    }
}

/// The popup is small and read at a glance, so each cause gets a short plain message.
fn notice_label(notice: &str) -> &'static str {
    const LABELS: [(&str, &str); 11] = [
        ("No text selected", "No text selected"),
        ("Protected", "Protected field"),
        ("Accessibility", "Allow Accessibility in settings"),
        ("Selection unavailable", "Copy text, then Read clipboard"),
        ("at most", "Selection is too long"),
        ("Clipboard", "Clipboard has no text"),
        ("Download", "Download this voice first"),
        ("Nothing to read", "Nothing to read"),
        ("Switching audio", "Switching audio output"),
        ("audio output", "No audio output"),
        ("Escape shortcut", "Esc unavailable. Use menu bar"),
    ];
    LABELS
        .iter()
        .find(|(cause, _)| notice.contains(cause))
        .map_or("Open settings for details", |(_, label)| label)
}

pub(super) fn visual(
    phase: Phase,
    capturing: bool,
    notice: Option<&str>,
    bars: &VecDeque<f32>,
    reduced: bool,
) -> gpui::AnyElement {
    let working = !capturing && phase == Phase::Preparing;
    let label = if capturing {
        "Capturing"
    } else if let Some(notice) = notice {
        notice_label(notice)
    } else {
        match phase {
            Phase::Reading => "Reading",
            Phase::Preparing => "Preparing…",
            Phase::Paused => "Paused",
            Phase::Failed => "Unavailable",
            Phase::Idle => "Ready",
        }
    };
    let mut meter = div()
        .id("speech-waveform")
        .role(Role::Status)
        .aria_label(label)
        .w(px(42.))
        .h(px(12.))
        .flex()
        .items_center()
        .gap(px(3.));
    for (index, energy) in bars.iter().enumerate() {
        let arming = capturing;
        let height = if arming {
            4.
        } else {
            3. + energy.powf(0.7) * 6.
        };
        let bar = div()
            .w(px(2.))
            .rounded(px(1.))
            .bg(if arming { rgb(0x8a96a3) } else { rgb(0x5ec8b8) })
            .opacity(if arming { 0.35 } else { 1. });
        let element = if arming && !reduced {
            let delay = [0., 75., 150., 225., 300., 225., 150., 75., 0.][index] / 900.;
            bar.with_animation(
                ("arming", index),
                Animation::new(std::time::Duration::from_millis(900)).repeat_synced(),
                move |bar, phase| {
                    let phase = (phase - delay + 1.) % 1.;
                    let p = if phase <= 0.5 {
                        phase * 2.
                    } else {
                        (1. - phase) * 2.
                    };
                    let p = motion::cubic_bezier(p, [0.42, 0., 0.58, 1.]);
                    bar.h(px(4. * (0.55 + 0.95 * p))).opacity(0.25 + 0.45 * p)
                },
            )
            .into_any_element()
        } else {
            // The reader samples the level at 30 Hz. Tweening between samples would
            // redraw at the display's 60–120 Hz for as long as speech plays.
            bar.h(px(height)).into_any_element()
        };
        meter = meter.child(element);
    }
    let leading = if working {
        let spinner = Icon::SpeechSpinner.element(12.).text_color(rgb(ACCENT));
        if reduced {
            spinner.into_any_element()
        } else {
            spinner
                .with_animation(
                    "speech-spinner",
                    Animation::new(std::time::Duration::from_millis(700)).repeat(),
                    |icon, phase| {
                        icon.with_transformation(gpui::Transformation::rotate(gpui::radians(
                            phase * std::f32::consts::TAU,
                        )))
                    },
                )
                .into_any_element()
        }
    } else if notice.is_some() {
        Icon::Voices
            .element(12.)
            .text_color(rgb(0xf1a718))
            .into_any_element()
    } else {
        div().into_any_element()
    };
    let center = if working || notice.is_some() || phase == Phase::Paused {
        let content = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(rgb(TEXT))
            .child(label);
        content
            .id("playback-status")
            .role(Role::Status)
            .aria_label(label)
            .into_any_element()
    } else {
        meter.into_any_element()
    };
    let pill = div()
        .id("player")
        .role(Role::Group)
        .aria_label("Speech playback")
        .h(px(28.))
        .relative()
        .bg(rgb(0x181c21).opacity(0.98))
        .border_1()
        .border_color(rgb(0x46505c))
        .px(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .text_color(rgb(TEXT))
        .font_family(".SystemUIFont")
        .when(working, |pill| {
            pill.child(div().absolute().left(px(14.)).top(px(8.)).child(leading))
        })
        .when(!working && notice.is_some(), |pill| {
            pill.child(Icon::Voices.element(12.).text_color(rgb(0xf1a718)))
        })
        .child(center);
    let width = if working {
        (label.chars().count() as f32 * 6.2 + 70.).clamp(126., 200.)
    } else if notice.is_some() {
        (label.chars().count() as f32 * 6.2 + 46.).clamp(100., 250.)
    } else {
        80.
    };
    let card = motion::tween("player-size", pill, width, 130, false, |view, width| {
        view.w(px(width)).rounded_full()
    })
    .curve(motion::Curve::Compact);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(card)
        .into_any_element()
}
