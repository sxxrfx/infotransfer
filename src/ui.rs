mod card;
mod controls;
mod dictionary_view;
mod footer;
mod motion;
mod player;
mod settings_view;
mod shortcuts;
mod slider;
#[cfg(feature = "visual-test")]
mod visual_check;
mod voice_picker;
use controls::AppButton;
use gpui::{
    App, Bounds, Context, Div, ElementId, FocusHandle, KeyBinding, Role, SharedString, Stateful,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, actions, div, point, prelude::*, px, rgb, size,
};
use infotransfer::{
    activation::{Activation, CaptureKind, CapturePlan, Effect},
    assets::{self, AssetPaths, SpeechModel},
    audio::clock_ns,
    design::{Icon, UiAssets},
    native,
    reader::{Origin, Phase, Reader},
    settings::{Settings, SettingsWriter},
    update::{self, Release},
    voices::{self, VOICES, VoiceFilter, VoiceSort},
};
use player::Player;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[cfg(feature = "visual-test")]
pub(crate) use visual_check::capture_component_previews;

actions!(infotransfer, [Tab, PreviousTab, OpenSettings, Quit]);
const BG: u32 = 0x1c2026;
const BORDER: u32 = 0x3a424d;
const TEXT: u32 = 0xf2f4f7;
const ACCENT: u32 = 0x5ec8b8;
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> AppButton {
    let id = id.into();
    let label = label.into();
    let inner = div()
        .id(id.clone())
        .role(Role::Button)
        .focusable()
        .tab_stop(true)
        .aria_label(label.clone())
        .px_4()
        .py(px(5.))
        .rounded_lg()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(0x808080).opacity(0.1))
        .text_size(px(13.125))
        .font_weight(gpui::FontWeight::MEDIUM)
        .line_height(px(18.75))
        .text_color(rgb(TEXT))
        .cursor_pointer()
        .focus_visible(|style| style.border_color(rgb(ACCENT)))
        .child(label);
    AppButton::new(id, inner, false)
}
fn tool_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
) -> AppButton {
    let id = id.into();
    let label = label.into();
    let inner = div()
        .id(id.clone())
        .role(Role::Button)
        .focusable()
        .tab_stop(true)
        .aria_label(label.clone())
        .h(px(30.))
        .px_3()
        .rounded_lg()
        .flex()
        .items_center()
        .gap_2()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_size(px(13.125))
        .line_height(px(18.75))
        .bg(if active {
            rgb(ACCENT).opacity(0.2)
        } else {
            rgb(0x808080).opacity(0.1)
        })
        .text_color(if active {
            rgb(ACCENT)
        } else {
            rgb(TEXT).opacity(0.6)
        })
        .cursor_pointer()
        .focus_visible(|style| style.border_1().border_color(rgb(ACCENT)))
        .child(label);
    AppButton::toolbar(id, inner, active)
}
fn row(label: &str) -> Div {
    row_content(setting_label(label))
}
fn setting_label(label: &str) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("label-{label}")))
        .role(Role::Label)
        .aria_label(label.to_owned())
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_size(px(13.125))
        .line_height(px(18.75))
        .child(label.to_owned())
}
fn row_content(label: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(45.))
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(label)
}
fn section(label: &str) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("section-{label}")))
        .role(Role::Heading)
        .aria_label(label.to_owned())
        .mb(px(8.))
        .px(px(8.))
        .text_size(px(13.))
        .line_height(px(20.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(0x9aa4b0))
        .child(label.to_owned())
}

fn group() -> Div {
    div()
        .border_1()
        .border_color(rgb(BORDER))
        .rounded(px(16.))
        .bg(rgb(0x808080).opacity(0.13))
}
fn view_pointer(window: &Window) -> Option<*mut c_void> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::AppKit(handle) => Some(handle.ns_view.as_ptr()),
        _ => None,
    }
}
#[derive(Default)]
struct CaptureState {
    revision: AtomicU64,
    active: AtomicBool,
    popup_dismissed: AtomicBool,
}
impl CaptureState {
    fn cancel(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.active.store(false, Ordering::Release);
    }
}
enum UiEvent {
    Captured {
        request: CapturePlan,
        result: anyhow::Result<native::Selection>,
    },
    Download {
        done: u64,
        total: u64,
    },
    DownloadFinished(Result<(), String>),
    UpdateDue,
    UpdateChecked {
        result: Result<Option<Release>, String>,
        manual: bool,
    },
    UpdateProgress {
        done: u64,
        total: u64,
    },
    UpdateDownloaded(Result<std::path::PathBuf, String>),
    UpdateInstalled(Result<(), String>),
}
#[derive(Clone, Default)]
enum UpdateState {
    #[default]
    Idle,
    Checking,
    Current,
    Available(Release),
    Downloading {
        release: Release,
        done: u64,
        total: u64,
        cancel: Arc<AtomicBool>,
    },
    /// Downloaded and checked; installing quits the app, so the user starts it.
    Ready {
        release: Release,
        archive: std::path::PathBuf,
    },
    /// A reading is playing; installing would stop it.
    ConfirmRestart {
        release: Release,
        archive: std::path::PathBuf,
    },
    Installing(String),
    /// Installed while a reading played; restart once it ends.
    RestartPending,
    Failed(String),
}
struct CaptureWorker {
    pending: Arc<Mutex<Option<CapturePlan>>>,
    wake: crossbeam_channel::Sender<()>,
}
impl CaptureWorker {
    fn new(events: async_channel::Sender<UiEvent>) -> Self {
        let pending = Arc::new(Mutex::new(None::<CapturePlan>));
        let work = pending.clone();
        let (wake, rx) = crossbeam_channel::bounded(1);
        std::thread::Builder::new()
            .name("selection-capture".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let request = work.lock().unwrap().take();
                    if let Some(request) = request {
                        let result = native::selection(request.pid);
                        if events
                            .try_send(UiEvent::Captured { request, result })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            })
            .expect("start selection capture");
        Self { pending, wake }
    }
    fn capture(&self, request: CapturePlan) {
        *self.pending.lock().unwrap() = Some(request);
        let _ = self.wake.try_send(());
    }
}
struct Desktop {
    reader: Reader,
    paths: AssetPaths,
    settings: Settings,
    writer: SettingsWriter,
    focus: FocusHandle,
    tab: usize,
    player: Option<WindowHandle<Player>>,
    capture: Arc<CaptureState>,
    capture_worker: CaptureWorker,
    activation: Activation,
    events: async_channel::Sender<UiEvent>,
    downloading: Option<DownloadProgress>,
    voice_filter: VoiceFilter,
    installed_only: bool,
    voice_sort: VoiceSort,
    voice_menu: Option<voice_picker::VoiceMenu>,
    voice_menu_cursor: usize,
    voice_menu_focus: FocusHandle,
    voice_filter_focus: FocusHandle,
    voice_sort_focus: FocusHandle,
    voice_menu_anchor: gpui::Point<gpui::Pixels>,
    slider_dragging: Option<slider::Adjustment>,
    voice_query: String,
    search_focused: bool,
    search_focus: FocusHandle,
    footer_open: bool,
    footer_cursor: usize,
    footer_focus: FocusHandle,
    footer_menu_focus: FocusHandle,
    footer_anchor: gpui::Point<gpui::Pixels>,
    download_cancel: Arc<AtomicBool>,
    last_phase: Phase,
    last_ready: bool,
    last_notice: Option<String>,
    update: UpdateState,
    update_announced: Option<String>,
    last_update_check: Option<std::time::Instant>,
}
struct DownloadProgress {
    model: SpeechModel,
    done: u64,
    total: u64,
}
impl Desktop {
    fn new(reader: Reader, paths: AssetPaths, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (events, rx) = async_channel::unbounded();
        let (reports, report_rx) = async_channel::unbounded();
        let settings = match Settings::load() {
            Ok((value, reset)) => {
                if let Some(message) = reset {
                    reader.notice(message);
                }
                value
            }
            Err(_) => {
                reader.notice(
                    "Settings could not be loaded. Defaults restored; a copy is in settings.json.bak."
                        .into(),
                );
                Settings::default()
            }
        };
        let _ = reader.set_preferences(settings.speech.clone());
        let writer = SettingsWriter::start(reports);
        let focus = cx.focus_handle();
        let search_focus = cx.focus_handle();
        cx.on_focus(&search_focus, window, |this, _, _| {
            if !this.search_focused {
                native::focus_search();
            }
        })
        .detach();
        window.set_rem_size(px(15.));
        window.focus(&focus, cx);
        if let Some(view) = view_pointer(window) {
            let address = view as usize;
            window.on_window_should_close(cx, move |_, _| {
                unsafe { native::window_visible(address as *mut c_void, false) };
                false
            });
        }
        let notifications = reader.notifications.clone();
        cx.spawn(async move |this, cx| {
            while notifications.recv().await.is_ok() {
                if this.update(cx, |this, cx| this.synchronize(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                if this
                    .update(cx, |this, cx| this.handle_ui(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(message) = report_rx.recv().await {
                if this
                    .update(cx, |this, _| this.reader.notice(message))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let result = Self {
            reader,
            paths,
            settings,
            writer,
            focus,
            tab: 0,
            player: None,
            capture: Arc::new(CaptureState::default()),
            capture_worker: CaptureWorker::new(events.clone()),
            activation: Activation::default(),
            events,
            downloading: None,
            voice_filter: VoiceFilter::All,
            installed_only: false,
            voice_sort: VoiceSort::Name,
            voice_menu: None,
            voice_menu_cursor: 0,
            voice_menu_focus: cx.focus_handle(),
            voice_filter_focus: cx.focus_handle(),
            voice_sort_focus: cx.focus_handle(),
            voice_menu_anchor: point(px(0.), px(0.)),
            slider_dragging: None,
            voice_query: String::new(),
            search_focused: false,
            search_focus,
            footer_open: false,
            footer_cursor: 0,
            footer_focus: cx.focus_handle(),
            footer_menu_focus: cx.focus_handle(),
            footer_anchor: point(px(0.), px(0.)),
            download_cancel: Arc::new(AtomicBool::new(false)),
            last_phase: Phase::Idle,
            last_ready: false,
            last_notice: None,
            update: UpdateState::Idle,
            update_announced: None,
            last_update_check: None,
        };
        if update::running_bundle().is_some() {
            // Development builds never update themselves; installed builds look daily.
            let events = result.events.clone();
            std::thread::Builder::new()
                .name("update-schedule".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(15));
                    while events.try_send(UiEvent::UpdateDue).is_ok() {
                        std::thread::sleep(std::time::Duration::from_secs(60 * 60));
                    }
                })
                .expect("start update schedule");
        }
        if !native::configure_activation(result.settings.activation_key) && native::ax_trusted() {
            result.reader.notice(
                "Keyboard monitoring is unavailable. Relaunch InfoTransfer to try again.".into(),
            );
        }
        result
    }
    fn preferences_changed(&self) {
        if let Err(error) = self.reader.set_preferences(self.settings.speech.clone()) {
            self.reader.notice(error.to_string());
        }
        self.writer.save(&self.settings);
    }
    fn synchronize(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.reader.snapshot();
        if snapshot.phase != self.last_phase
            || snapshot.model_ready != self.last_ready
            || snapshot.notice != self.last_notice
        {
            let active = matches!(
                snapshot.phase,
                Phase::Preparing | Phase::Reading | Phase::Paused
            );
            native::playback_state(
                snapshot.phase == Phase::Paused,
                active,
                active || snapshot.notice.is_some(),
            );
            self.last_phase = snapshot.phase;
            self.last_ready = snapshot.model_ready;
            self.last_notice = snapshot.notice;
            if matches!(self.update, UpdateState::RestartPending) && !active {
                self.restart(cx);
                return;
            }
            cx.notify();
        }
        self.update_player(cx);
    }
    fn toggle_playback(&self) {
        self.capture.popup_dismissed.store(false, Ordering::Release);
        self.reader.pause_or_resume();
    }
    fn update_player(&self, cx: &mut Context<Self>) {
        if let Some(player) = self.player {
            let _ = player.update(cx, |player, _, cx| player.synchronize(cx));
        }
    }
    fn preview_voice(&mut self, voice: u32, cx: &mut Context<Self>) {
        self.cancel_capture();
        self.capture.popup_dismissed.store(false, Ordering::Release);
        native::player_position(0);
        self.reader.preview(voice);
        self.update_player(cx);
    }
    fn cancel_capture(&mut self) {
        self.activation.cancel();
        self.capture.cancel();
    }
    fn begin_capture(
        &mut self,
        event: native::Event,
        feedback: bool,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if !native::ax_trusted() {
            self.reader.notice(
                "Allow Accessibility access to read selections. Read clipboard remains available."
                    .into(),
            );
            return None;
        }
        let revision = self.capture.revision.fetch_add(1, Ordering::AcqRel) + 1;
        self.capture.active.store(false, Ordering::Release);
        if feedback {
            self.show_capture(event.pid, cx);
        }
        Some(revision)
    }
    fn show_capture(&self, pid: i32, cx: &mut Context<Self>) {
        self.capture.popup_dismissed.store(false, Ordering::Release);
        self.capture.active.store(true, Ordering::Release);
        native::player_position(native::source_display(pid));
        self.update_player(cx);
    }
    fn apply_activation(&mut self, effect: Option<Effect>, cx: &mut Context<Self>) {
        match effect {
            Some(Effect::Capture(request)) => {
                if request.kind != CaptureKind::Baseline {
                    self.show_capture(request.pid, cx);
                }
                self.capture_worker.capture(request);
            }
            Some(Effect::Cancel) => {
                self.cancel_capture();
                self.update_player(cx);
            }
            None => (),
        }
    }
    fn start_capture(&mut self, event: native::Event, cx: &mut Context<Self>) {
        self.activation.cancel();
        if let Some(revision) = self.begin_capture(event, true, cx) {
            self.capture_worker.capture(CapturePlan {
                pid: event.pid,
                revision,
                activation: event.activation_ns,
                kind: CaptureKind::Tap,
                baseline: None,
            });
        }
    }
    fn handle_native(&mut self, event: native::Event, window: &mut Window, cx: &mut Context<Self>) {
        match event.action {
            1 => self.start_capture(event, cx),
            20 => {
                self.reader.prewarm();
                if !self.activation.active()
                    && let Some(revision) = self.begin_capture(event, false, cx)
                {
                    let effect = self
                        .activation
                        .press(event.pid, event.activation_ns, revision);
                    self.apply_activation(effect, cx);
                }
            }
            21 => {
                let participating = self.activation.active();
                let effect = self.activation.release(event.pid, event.activation_ns);
                if participating && effect.is_none() {
                    // A held release may still be waiting for its baseline AX read.
                    self.show_capture(event.pid, cx);
                }
                self.apply_activation(effect, cx);
            }
            22 => {
                self.cancel_capture();
                self.update_player(cx);
            }
            23 => {
                if let Some(entry) = native::dictionary_result() {
                    let mut dictionary = self.settings.speech.dictionary.clone();
                    if event.pid == 0 {
                        dictionary.push(entry);
                    } else if let Some(existing) = dictionary.get_mut(event.pid as usize - 1) {
                        *existing = entry;
                    } else {
                        native::dictionary_feedback(Some(
                            "This entry changed. Cancel and try again.",
                        ));
                        return;
                    }
                    match infotransfer::dictionary::validate(&dictionary) {
                        Ok(()) => {
                            self.settings.speech.dictionary = dictionary;
                            self.preferences_changed();
                            native::dictionary_feedback(None);
                            cx.notify();
                        }
                        Err(error) => native::dictionary_feedback(Some(&error.to_string())),
                    }
                }
            }
            2 => {
                self.capture.popup_dismissed.store(false, Ordering::Release);
                self.cancel_capture();
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    native::player_position(native::source_display(event.pid));
                    let _ = self
                        .reader
                        .read_at(text, Origin::Clipboard, event.activation_ns);
                } else {
                    self.reader.notice("Clipboard contains no text".into());
                }
            }
            3 => self.toggle_playback(),
            4 => {
                self.cancel_capture();
                self.reader.stop();
                self.update_player(cx);
            }
            5 => {
                if let Some(view) = view_pointer(window) {
                    unsafe { native::window_visible(view, true) };
                }
                cx.activate(true);
                cx.notify();
            }
            6 => {
                self.cancel_capture();
                self.reader.shutdown();
                cx.quit();
            }
            7 => {
                self.cancel_capture();
                self.reader.pause();
                self.update_player(cx);
            }
            12 => {
                self.cancel_capture();
                self.capture.popup_dismissed.store(true, Ordering::Release);
                self.reader.stop();
                self.update_player(cx);
            }
            13 => {
                cx.set_reduce_motion(native::reduced_motion());
                cx.notify();
            }
            14 => {
                self.voice_query = native::search_text();
                cx.notify();
            }
            15 | 16 => {
                self.search_focused = event.action == 15;
                if self.search_focused {
                    window.focus(&self.search_focus, cx);
                }
                cx.notify();
            }
            17 => window.focus(&self.voice_filter_focus, cx),
            18 => {
                window.focus(&self.search_focus, cx);
                window.focus_prev(cx);
            }
            19 => {
                if !native::configure_activation(self.settings.activation_key)
                    && native::ax_trusted()
                {
                    self.reader.notice(
                        "Keyboard monitoring is unavailable. Relaunch InfoTransfer to try again."
                            .into(),
                    );
                }
                cx.notify();
            }
            _ => {}
        }
    }
    fn handle_ui(&mut self, event: UiEvent, cx: &mut Context<Self>) {
        match event {
            UiEvent::Captured { request, result } => {
                if request.revision != self.capture.revision.load(Ordering::Acquire) {
                    return;
                }
                if request.kind == CaptureKind::Baseline {
                    let effect = self.activation.baseline(
                        request.revision,
                        result
                            .as_ref()
                            .ok()
                            .map(|selection| selection.fingerprint()),
                    );
                    self.apply_activation(effect, cx);
                    return;
                }
                self.capture.active.store(false, Ordering::Release);
                match result {
                    Ok(selection) => {
                        let changed = request
                            .baseline
                            .as_ref()
                            .is_none_or(|prior| selection.fingerprint().changed_from(prior));
                        if request.kind != CaptureKind::Hold
                            || (changed && !selection.text.trim().is_empty())
                        {
                            let _ = self.reader.read_at(
                                selection.text,
                                Origin::Selection,
                                request.activation,
                            );
                        }
                    }
                    Err(error) if request.kind != CaptureKind::Hold => {
                        self.reader.notice(error.to_string())
                    }
                    Err(_) => (),
                }
                self.update_player(cx);
            }
            UiEvent::Download { done, total } => {
                if let Some(progress) = &mut self.downloading {
                    progress.done = done;
                    progress.total = total;
                }
                cx.notify();
            }
            UiEvent::DownloadFinished(result) => {
                self.downloading = None;
                match result {
                    Ok(()) => self.reader.retry_load(),
                    Err(error) => self.reader.notice(error),
                }
                cx.notify();
            }
            UiEvent::UpdateDue => {
                let day = std::time::Duration::from_secs(24 * 60 * 60);
                if self.settings.check_updates
                    && self
                        .last_update_check
                        .is_none_or(|time| time.elapsed() >= day)
                {
                    self.check_for_update(false, cx);
                }
            }
            UiEvent::UpdateChecked { result, manual } => {
                self.update = match result {
                    Ok(Some(release)) => {
                        if !manual && self.update_announced.as_ref() != Some(&release.version) {
                            self.reader.notice(format!(
                                "InfoTransfer {} is available. Install it in Settings → About.",
                                release.version
                            ));
                            self.update_announced = Some(release.version.clone());
                        }
                        UpdateState::Available(release)
                    }
                    Ok(None) => UpdateState::Current,
                    // A background check fails quietly, for example while offline.
                    Err(error) if manual => UpdateState::Failed(error),
                    Err(_) => UpdateState::Idle,
                };
                cx.notify();
            }
            UiEvent::UpdateProgress { done, total } => {
                if let UpdateState::Downloading {
                    done: current,
                    total: size,
                    ..
                } = &mut self.update
                {
                    (*current, *size) = (done, total);
                }
                cx.notify();
            }
            UiEvent::UpdateDownloaded(result) => {
                if let UpdateState::Downloading {
                    release, cancel, ..
                } = self.update.clone()
                {
                    self.update = match result {
                        Ok(archive) => UpdateState::Ready { release, archive },
                        Err(_) if cancel.load(Ordering::Acquire) => UpdateState::Available(release),
                        Err(error) => UpdateState::Failed(error),
                    };
                    cx.notify();
                }
            }
            UiEvent::UpdateInstalled(result) => match result {
                Ok(()) if self.reading_active() => {
                    self.update = UpdateState::RestartPending;
                    cx.notify();
                }
                Ok(()) => self.restart(cx),
                Err(error) => {
                    self.update = UpdateState::Failed(error);
                    cx.notify();
                }
            },
        }
    }
    fn check_for_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !matches!(
            self.update,
            UpdateState::Idle
                | UpdateState::Current
                | UpdateState::Available(_)
                | UpdateState::Failed(_)
        ) {
            return;
        }
        self.update = UpdateState::Checking;
        self.last_update_check = Some(std::time::Instant::now());
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let result = update::check().map_err(|error| error.to_string());
                let _ = events.try_send(UiEvent::UpdateChecked { result, manual });
            })
            .expect("start update check");
        cx.notify();
    }
    fn reading_active(&self) -> bool {
        matches!(
            self.reader.snapshot().phase,
            Phase::Preparing | Phase::Reading | Phase::Paused
        )
    }
    /// Downloads and checks the available update. Installing is a separate step.
    fn download_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = self.update.clone() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.update = UpdateState::Downloading {
            release: release.clone(),
            done: 0,
            total: release.size,
            cancel: cancel.clone(),
        };
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("update-download".into())
            .spawn(move || {
                let directory = assets::data_dir().join("updates");
                let _ = std::fs::remove_dir_all(&directory);
                let result = update::download(&release, &directory, |done, total| {
                    let _ = events.try_send(UiEvent::UpdateProgress { done, total });
                    !cancel.load(Ordering::Acquire)
                })
                .map_err(|error| error.to_string());
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&directory);
                }
                let _ = events.try_send(UiEvent::UpdateDownloaded(result));
            })
            .expect("start update download");
        cx.notify();
    }
    fn cancel_update_download(&mut self, cx: &mut Context<Self>) {
        if let UpdateState::Downloading { cancel, .. } = &self.update {
            cancel.store(true, Ordering::Release);
        }
        cx.notify();
    }
    /// Installs a downloaded update, asking first if that would stop a reading.
    fn install_update(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        let (UpdateState::Ready { release, archive }
        | UpdateState::ConfirmRestart { release, archive }) = self.update.clone()
        else {
            return;
        };
        let Some(bundle) = update::running_bundle() else {
            return;
        };
        if !confirmed && self.reading_active() {
            self.update = UpdateState::ConfirmRestart { release, archive };
            cx.notify();
            return;
        }
        self.update = UpdateState::Installing(release.version.clone());
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("update-install".into())
            .spawn(move || {
                let result =
                    update::install(&archive, &release, &bundle).map_err(|error| error.to_string());
                let _ = std::fs::remove_dir_all(assets::data_dir().join("updates"));
                let _ = events.try_send(UiEvent::UpdateInstalled(result));
            })
            .expect("start update install");
        cx.notify();
    }
    fn postpone_update(&mut self, cx: &mut Context<Self>) {
        if let UpdateState::ConfirmRestart { release, archive } = self.update.clone() {
            self.update = UpdateState::Ready { release, archive };
            cx.notify();
        }
    }
    /// Relaunches the newly installed version.
    fn restart(&mut self, cx: &mut Context<Self>) {
        let Some(bundle) = update::running_bundle() else {
            return;
        };
        if let Err(error) = update::relaunch_after_exit(&bundle) {
            self.update = UpdateState::Failed(format!(
                "Updated. Quit and open InfoTransfer again to finish. ({error})"
            ));
            cx.notify();
            return;
        }
        self.cancel_capture();
        self.reader.shutdown();
        cx.quit();
    }
    fn download(&mut self, cx: &mut Context<Self>) {
        self.download_voice(self.settings.speech.voice, cx);
    }
    fn download_voice(&mut self, voice: u32, cx: &mut Context<Self>) {
        let paths = self.reader.assets_for_voice(voice);
        if let Some(progress) = &self.downloading {
            if progress.model == paths.model {
                self.download_cancel.store(true, Ordering::Release);
            } else {
                self.reader
                    .notice("Another voice download is in progress".into());
            }
            return;
        }
        self.downloading = Some(DownloadProgress {
            model: paths.model,
            done: 0,
            total: 1,
        });
        self.download_cancel.store(false, Ordering::Release);
        let events = self.events.clone();
        let cancel = self.download_cancel.clone();
        std::thread::Builder::new()
            .name("voice-download".into())
            .spawn(move || {
                let result = assets::prepare(paths.model, &paths.speech, |_, done, total| {
                    let _ = events.try_send(UiEvent::Download { done, total });
                    !cancel.load(Ordering::Acquire)
                })
                .map_err(|error| {
                    if cancel.load(Ordering::Acquire) {
                        "Download canceled".into()
                    } else {
                        format!("Voice setup failed: {error}. Check your connection and disk space, then retry.")
                    }
                });
                let _ = events.try_send(UiEvent::DownloadFinished(result));
            })
            .expect("start voice download");
        cx.notify();
    }
}

pub fn run() {
    let paths = AssetPaths::discover().expect("locate speech resources");
    let reader = Reader::start(paths.clone());
    let (app_events, app_rx) = async_channel::unbounded();
    let reopen_events = app_events.clone();
    let application = gpui_platform::application().with_assets(UiAssets);
    application.on_reopen(move |_| {
        let _ = reopen_events.try_send(native::Event {
            action: 5,
            pid: 0,
            activation_ns: clock_ns(),
        });
    });
    application.run(move |cx: &mut App| {
        cx.set_reduce_motion(native::reduced_motion());
        cx.bind_keys([
            KeyBinding::new(
                "tab",
                Tab,
                Some("InfoTransferSettings && !NativeVoiceSearch"),
            ),
            KeyBinding::new(
                "shift-tab",
                PreviousTab,
                Some("InfoTransferSettings && !NativeVoiceSearch"),
            ),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus([gpui::Menu::new("InfoTransfer").items([
            gpui::MenuItem::action("Settings…", OpenSettings),
            gpui::MenuItem::separator(),
            gpui::MenuItem::action("Quit InfoTransfer", Quit),
        ])]);
        let native_events = native::install();
        let settings = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(680.), px(570.)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("InfoTransfer".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(8.), px(8.))),
                    }),
                    window_min_size: Some(size(px(680.), px(570.))),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Desktop::new(reader.clone(), paths.clone(), window, cx)),
            )
            .expect("open settings");
        cx.on_action(move |_: &OpenSettings, cx| {
            let _ = settings.update(cx, |_, window, cx| {
                if let Some(view) = view_pointer(window) {
                    unsafe { native::window_visible(view, true) };
                }
                cx.activate(true);
            });
        });
        let capture = settings.read(cx).expect("settings view").capture.clone();
        let player = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(260.), px(28.)),
                        cx,
                    ))),
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    focus: false,
                    show: false,
                    is_resizable: false,
                    is_minimizable: false,
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                |window, cx| {
                    if let Some(view) = view_pointer(window) {
                        unsafe { native::adopt_player(view) };
                        native::player_position(0);
                        native::player_visible(false);
                    }
                    cx.new(|_| Player::new(reader.clone(), capture))
                },
            )
            .expect("open compact player");
        settings
            .update(cx, |desktop, window, cx| {
                desktop.player = Some(player);
                desktop.synchronize(cx);
                // Once set up, this is a quiet menu bar app; only first run needs the window.
                let voice = desktop.settings.speech.voice;
                if native::ax_trusted()
                    && desktop.reader.assets_for_voice(voice).ready()
                    && let Some(view) = view_pointer(window)
                {
                    unsafe { native::window_visible(view, false) };
                }
            })
            .expect("connect player");
        for receiver in [native_events, app_rx] {
            cx.spawn(async move |cx| {
                while let Ok(event) = receiver.recv().await {
                    if settings
                        .update(cx, |desktop, window, cx| {
                            desktop.handle_native(event, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let quitting = reader.clone();
        cx.on_app_quit(move |_| {
            quitting.shutdown();
            async {}
        })
        .detach();
        if !native::ax_trusted() {
            cx.activate(true);
        }
    });
}
