use crate::{
    activation::{ActivationKey, KeyEdge, KeyPreference, ModifierGesture},
    audio::clock_ns,
};
use anyhow::{Context as _, Result, bail};
use std::{
    ffi::{CStr, c_char, c_void},
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Copy)]
pub struct Event {
    pub action: i32,
    pub pid: i32,
    pub activation_ns: u64,
}
static EVENTS: OnceLock<async_channel::Sender<Event>> = OnceLock::new();
static KEY: Mutex<ModifierGesture> = Mutex::new(ModifierGesture::new(ActivationKey::Fn));
static EDITOR_WORDS: Mutex<Vec<crate::dictionary::WordReplacement>> = Mutex::new(Vec::new());
unsafe extern "C" {
    fn it_native_activate_existing() -> i32;
    fn it_native_install(
        callback: extern "C" fn(i32, i32),
        key_callback: extern "C" fn(u16, u8, i32, i32),
    );
    fn it_native_activation_monitor() -> i32;
    fn it_native_fn_unassigned() -> i32;
    fn it_native_escape(active: i32) -> i32;
    fn it_native_playback_state(paused: i32, active: i32, can_stop: i32);
    fn it_native_ax_trusted() -> i32;
    fn it_native_open_at_login() -> i32;
    fn it_native_set_open_at_login(enabled: i32) -> i32;
    fn it_native_dictionary_edit(
        view: *mut c_void,
        index: i32,
        word: *const c_char,
        replacement: *const c_char,
        match_case: i32,
        check: extern "C" fn(*const c_char, i32, *mut c_char, usize) -> i32,
    );
    fn it_native_dictionary_result() -> *mut c_char;
    fn it_native_dictionary_feedback(error: *const c_char);
    fn it_native_request_ax();
    fn it_native_access_settings();
    fn it_native_selection(
        pid: i32,
        output: *mut *mut c_char,
        start: *mut i64,
        length: *mut i64,
    ) -> i32;
    fn it_native_player(view: *mut c_void);
    fn it_native_player_visible(visible: i32);
    fn it_native_window_visible(view: *mut c_void, visible: i32);
    fn it_native_player_position(display: u32);
    fn it_native_source_display(pid: i32) -> u32;
    fn it_native_reduced_motion() -> i32;
    fn it_native_search_layout(
        view: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        clip_x: f64,
        clip_y: f64,
        clip_width: f64,
        clip_height: f64,
    );
    fn it_native_search_visible(visible: i32);
    fn it_native_search_focus();
    fn it_native_search_text() -> *mut c_char;
    fn it_native_search_clear();
}
pub fn activate_existing() -> bool {
    unsafe { it_native_activate_existing() != 0 }
}
extern "C" fn receive(action: i32, pid: i32) {
    if matches!(action, 7 | 12 | 22) {
        KEY.lock().unwrap().cancel();
    }
    if let Some(events) = EVENTS.get() {
        let _ = events.try_send(Event {
            action,
            pid,
            activation_ns: clock_ns(),
        });
    }
}
extern "C" fn receive_key(code: u16, flags: u8, key_down: i32, pid: i32) {
    let edge = KEY.lock().unwrap().event(code, flags, key_down != 0);
    if let Some(edge) = edge {
        receive(
            match edge {
                KeyEdge::Press => 20,
                KeyEdge::Release => 21,
                KeyEdge::Cancel => 22,
            },
            pid,
        );
    }
}

// All AppKit operations below run on GPUI's main thread. Selection alone is
// worker-safe; the native callback never captures or logs the user's text.
pub fn install() -> async_channel::Receiver<Event> {
    let (tx, rx) = async_channel::unbounded();
    assert!(
        EVENTS.set(tx).is_ok(),
        "Native integration initialized once"
    );
    unsafe { it_native_install(receive, receive_key) };
    rx
}
pub fn configure_activation(preference: KeyPreference) -> bool {
    let key = preference.resolve(fn_unassigned());
    let mut state = KEY.lock().unwrap();
    if state.key != key {
        *state = ModifierGesture::new(key);
        drop(state);
        receive(22, 0);
    } else {
        drop(state);
    }
    unsafe { it_native_activation_monitor() != 0 }
}
pub fn activation_key() -> ActivationKey {
    KEY.lock().unwrap().key
}
pub fn fn_unassigned() -> bool {
    unsafe { it_native_fn_unassigned() == 1 }
}
pub fn escape_active(active: bool) -> bool {
    unsafe { it_native_escape(active as i32) != 0 }
}
pub fn playback_state(paused: bool, active: bool, can_stop: bool) {
    unsafe { it_native_playback_state(paused as i32, active as i32, can_stop as i32) }
}
pub fn ax_trusted() -> bool {
    unsafe { it_native_ax_trusted() != 0 }
}
pub fn open_at_login() -> bool {
    unsafe { it_native_open_at_login() != 0 }
}
pub fn set_open_at_login(enabled: bool) -> bool {
    unsafe { it_native_set_open_at_login(enabled as i32) != 0 }
}
/// # Safety
/// `view` must be a live GPUI AppKit view, called on its main thread.
pub unsafe fn edit_dictionary(
    view: *mut c_void,
    index: Option<usize>,
    entries: &[crate::dictionary::WordReplacement],
) {
    *EDITOR_WORDS.lock().unwrap() = entries.to_vec();
    let entry = index.and_then(|index| entries.get(index));
    let word = std::ffi::CString::new(entry.map_or("", |entry| entry.word.as_str()))
        .expect("validated dictionary word");
    let replacement = std::ffi::CString::new(entry.map_or("", |entry| entry.replacement.as_str()))
        .expect("validated replacement");
    unsafe {
        it_native_dictionary_edit(
            view,
            index.map_or(0, |index| index as i32 + 1),
            word.as_ptr(),
            replacement.as_ptr(),
            entry.is_some_and(|entry| entry.match_case) as i32,
            check_dictionary,
        )
    };
}
extern "C" fn check_dictionary(
    json: *const c_char,
    index: i32,
    error: *mut c_char,
    capacity: usize,
) -> i32 {
    let result: anyhow::Result<()> = (|| {
        let entry = serde_json::from_slice::<crate::dictionary::WordReplacement>(
            unsafe { CStr::from_ptr(json) }.to_bytes(),
        )
        .context("This replacement could not be read. Cancel and try again.")?;
        let mut entries = EDITOR_WORDS.lock().unwrap().clone();
        if index == 0 {
            entries.push(entry);
        } else {
            *entries
                .get_mut(index as usize - 1)
                .context("This entry changed. Cancel and try again.")? = entry;
        }
        crate::dictionary::validate(&entries)
    })();
    if let Err(message) = result {
        let text = message.to_string();
        let count = text.len().min(capacity.saturating_sub(1));
        if capacity > 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr(), error.cast(), count);
                *error.add(count) = 0;
            }
        }
        0
    } else {
        1
    }
}
pub fn dictionary_result() -> Option<crate::dictionary::WordReplacement> {
    let raw = unsafe { it_native_dictionary_result() };
    if raw.is_null() {
        return None;
    }
    let result = serde_json::from_slice(unsafe { CStr::from_ptr(raw) }.to_bytes()).ok();
    unsafe { libc::free(raw.cast()) };
    result
}
pub fn dictionary_feedback(error: Option<&str>) {
    let message = error.map(|error| std::ffi::CString::new(error).expect("content-free error"));
    unsafe {
        it_native_dictionary_feedback(
            message
                .as_ref()
                .map_or(std::ptr::null(), |text| text.as_ptr()),
        )
    };
}
pub fn request_ax() {
    unsafe { it_native_request_ax() }
}
pub fn access_settings() {
    unsafe { it_native_access_settings() }
}
pub fn reduced_motion() -> bool {
    unsafe { it_native_reduced_motion() != 0 }
}
/// # Safety
/// `view` must be the live settings NSView, used only on the main thread.
pub unsafe fn search_layout(view: *mut c_void, bounds: [f64; 4], clip: [f64; 4]) {
    unsafe {
        it_native_search_layout(
            view, bounds[0], bounds[1], bounds[2], bounds[3], clip[0], clip[1], clip[2], clip[3],
        )
    }
}
pub fn search_visible(visible: bool) {
    unsafe { it_native_search_visible(visible as i32) }
}
pub fn focus_search() {
    unsafe { it_native_search_focus() }
}
pub fn clear_search() {
    unsafe { it_native_search_clear() }
}
pub fn search_text() -> String {
    let raw = unsafe { it_native_search_text() };
    if raw.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(raw.cast()) };
    text
}
pub fn source_display(pid: i32) -> u32 {
    unsafe { it_native_source_display(pid) }
}
pub fn player_position(display: u32) {
    unsafe { it_native_player_position(display) }
}
pub fn player_visible(visible: bool) {
    unsafe { it_native_player_visible(visible as i32) }
}
/// # Safety
/// `view` must be a live AppKit NSView on the main thread.
pub unsafe fn adopt_player(view: *mut c_void) {
    unsafe { it_native_player(view) }
}
/// # Safety
/// `view` must be a live AppKit NSView on the main thread.
pub unsafe fn window_visible(view: *mut c_void, visible: bool) {
    unsafe { it_native_window_visible(view, visible as i32) }
}
pub struct Selection {
    pub text: String,
    pub range: Option<(i64, i64)>,
}
impl Selection {
    pub fn fingerprint(&self) -> crate::activation::SelectionFingerprint {
        use sha2::{Digest, Sha256};
        crate::activation::SelectionFingerprint {
            digest: Sha256::digest(self.text.as_bytes()).into(),
            range: self.range,
        }
    }
}
pub fn selection(pid: i32) -> Result<Selection> {
    let mut output = std::ptr::null_mut();
    let (mut start, mut length) = (-1, -1);
    let result = unsafe { it_native_selection(pid, &mut output, &mut start, &mut length) };
    if result != 0 {
        bail!(match result {
            1 => "Allow Accessibility access to read selections",
            4 => "Protected fields cannot be read",
            5 => "Select at most 100,000 characters",
            _ => "Selection unavailable. Copy the text, then choose Read clipboard.",
        });
    }
    if output.is_null() {
        bail!("No text selected");
    }
    let bytes = unsafe { CStr::from_ptr(output).to_bytes().to_vec() };
    unsafe { libc::free(output.cast()) };
    Ok(Selection {
        text: String::from_utf8(bytes)?,
        range: (start >= 0 && length >= 0).then_some((start, length)),
    })
}
