use crate::{assets::data_dir, reader::Preferences};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub speech: Preferences,
    pub activation_key: crate::activation::KeyPreference,
    pub sound_defaults_version: u8,
    /// Look for signed updates on GitHub once a day.
    pub check_updates: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            speech: Default::default(),
            activation_key: Default::default(),
            sound_defaults_version: 1,
            check_updates: true,
        }
    }
}
impl Settings {
    /// Loads saved settings. A value that cannot be used is reset on its own, and the
    /// message names what was reset.
    pub fn load() -> Result<(Self, Option<String>)> {
        let path = data_dir().join("settings.json");
        if !path.exists() {
            return Ok((Self::default(), None));
        }
        let bytes = fs::read(&path)?;
        let keep_copy = || {
            // The next save overwrites this file, so keep the user's dictionary recoverable.
            let _ = fs::write(path.with_extension("json.bak"), &bytes);
        };
        let (value, reset) = Self::from_json(&bytes).inspect_err(|_| keep_copy())?;
        if reset.is_empty() {
            return Ok((value, None));
        }
        keep_copy();
        Ok((
            value,
            Some(format!(
                "Reset {} that could not be loaded. A copy is in settings.json.bak.",
                reset.join(", ")
            )),
        ))
    }
    fn from_json(bytes: &[u8]) -> Result<(Self, Vec<&'static str>)> {
        let raw: serde_json::Value = serde_json::from_slice(bytes)?;
        let legacy = raw
            .get("sound_defaults_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            == 0;
        let mut reset = Vec::new();
        let mut value: Self = match serde_json::from_value(raw.clone()) {
            Ok(value) => value,
            Err(_) => {
                reset.push("unreadable settings");
                Self::lenient(&raw)
            }
        };
        // Replace the previous automatic gap defaults once; preserve custom gaps.
        if legacy
            && value.speech.sound.sentence_pause_ms == 300
            && value.speech.sound.paragraph_pause_ms == 650
        {
            value.speech.sound.sentence_pause_ms = 0;
            value.speech.sound.paragraph_pause_ms = 0;
        }
        value.sound_defaults_version = 1;
        // One unusable value resets that value, not the whole saved configuration.
        if value.speech.sound.validate().is_err() {
            value.speech.sound = Default::default();
            reset.push("sound settings");
        }
        let mut dictionary = Vec::new();
        for entry in std::mem::take(&mut value.speech.dictionary) {
            dictionary.push(entry);
            if crate::dictionary::validate(&dictionary).is_err() {
                dictionary.pop();
                if !reset.contains(&"dictionary entries") {
                    reset.push("dictionary entries");
                }
            }
        }
        value.speech.dictionary = dictionary;
        if !crate::voices::available(value.speech.voice) {
            value.speech.voice = 0;
        }
        if !(0.75..=2.).contains(&value.speech.speed) {
            value.speech.speed = 1.;
            reset.push("speed");
        }
        Ok((value, reset))
    }
    /// Keeps every field, and every dictionary entry, that still has a usable type.
    fn lenient(raw: &serde_json::Value) -> Self {
        let mut value = serde_json::to_value(Self::default()).expect("serialize settings");
        merge_fields::<Self>(&mut value, raw, "speech");
        if let Some(saved) = raw.get("speech") {
            let mut speech = value["speech"].take();
            merge_fields::<Preferences>(&mut speech, saved, "dictionary");
            if let Some(entries) = saved.get("dictionary").and_then(|value| value.as_array()) {
                speech["dictionary"] = entries
                    .iter()
                    .filter(|entry| {
                        serde_json::from_value::<crate::dictionary::WordReplacement>(
                            (*entry).clone(),
                        )
                        .is_ok()
                    })
                    .cloned()
                    .collect();
            }
            value["speech"] = speech;
        }
        serde_json::from_value(value).unwrap_or_default()
    }
}

/// Copies each saved field into `target` only while `T` still deserializes.
fn merge_fields<T: serde::de::DeserializeOwned>(
    target: &mut serde_json::Value,
    saved: &serde_json::Value,
    skip: &str,
) {
    let (Some(saved), Some(object)) = (saved.as_object(), target.as_object_mut()) else {
        return;
    };
    for (key, item) in saved {
        if key == skip {
            continue;
        }
        let previous = object.insert(key.clone(), item.clone());
        if serde_json::from_value::<T>(serde_json::Value::Object(object.clone())).is_err() {
            match previous {
                Some(previous) => object.insert(key.clone(), previous),
                None => object.remove(key),
            };
        }
    }
}

pub struct SettingsWriter {
    tx: crossbeam_channel::Sender<Settings>,
}
impl SettingsWriter {
    pub fn start(report: async_channel::Sender<String>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<Settings>();
        std::thread::Builder::new()
            .name("settings-storage".into())
            .spawn(move || {
                while let Ok(mut value) = rx.recv() {
                    if let Some(latest) = rx.try_iter().last() {
                        value = latest;
                    }
                    if save(&value).is_err() {
                        let _ = report.try_send(
                            "Settings could not be saved. Check available disk space.".into(),
                        );
                    }
                }
            })
            .expect("start settings writer");
        Self { tx }
    }
    pub fn save(&self, settings: &Settings) {
        let _ = self.tx.send(settings.clone());
    }
}
fn save(settings: &Settings) -> Result<()> {
    let directory = data_dir();
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let path: PathBuf = directory.join("settings.json");
    let staging = directory.join("settings.json.new");
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&staging)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer(&file, settings)?;
    file.sync_all()?;
    fs::rename(staging, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_shortcuts_are_removed_without_losing_speech_preferences() {
        let saved = r#"{"speech":{"voice":8,"speed":1.25,"memory_saver":true},"bindings":[{"key":11,"modifiers":8},{"key":0,"modifiers":3},{"key":49,"modifiers":8},{"key":53,"modifiers":8}]}"#;
        let settings: Settings = serde_json::from_str(saved).unwrap();
        // Index 8 was Heart in the old catalog; removed voices and indexes load as Heart.
        assert_eq!(settings.speech.voice, 0);
        assert_eq!(settings.speech.speed, 1.25);
        assert!(settings.speech.memory_saver);
        assert_eq!(
            settings.activation_key,
            crate::activation::KeyPreference::Auto
        );
        let persisted = serde_json::to_value(settings).unwrap();
        assert!(persisted.get("bindings").is_none());
    }
    #[test]
    fn one_invalid_value_does_not_reset_the_dictionary() {
        let saved = br#"{"speech":{"voice":99,"speed":9.0,"dictionary":[{"word":"k8s","replacement":"kubernetes","match_case":false}]}}"#;
        let loaded = Settings::from_json(saved).unwrap().0;
        assert_eq!(loaded.speech.voice, 0);
        assert_eq!(loaded.speech.speed, 1.);
        assert_eq!(loaded.speech.dictionary[0].replacement, "kubernetes");
    }
    #[test]
    fn activation_key_choice_is_persisted() {
        let settings = Settings {
            activation_key: crate::activation::KeyPreference::RightOption,
            ..Default::default()
        };
        let saved = serde_json::to_vec(&settings).unwrap();
        let loaded: Settings = serde_json::from_slice(&saved).unwrap();
        assert_eq!(loaded.activation_key, settings.activation_key);
    }
    #[test]
    fn natural_pacing_migrates_only_the_old_automatic_gaps() {
        let legacy = br#"{"speech":{"sound":{"sentence_pause_ms":300,"paragraph_pause_ms":650}}}"#;
        let loaded = Settings::from_json(legacy).unwrap().0;
        assert_eq!(loaded.speech.sound.sentence_pause_ms, 0);
        assert_eq!(loaded.speech.sound.paragraph_pause_ms, 0);
        let custom = br#"{"speech":{"sound":{"sentence_pause_ms":200,"paragraph_pause_ms":450}}}"#;
        assert_eq!(
            Settings::from_json(custom)
                .unwrap()
                .0
                .speech
                .sound
                .sentence_pause_ms,
            200
        );
        let saved = br#"{"sound_defaults_version":1,"speech":{"sound":{"sentence_pause_ms":300,"paragraph_pause_ms":650}}}"#;
        assert_eq!(
            Settings::from_json(saved)
                .unwrap()
                .0
                .speech
                .sound
                .sentence_pause_ms,
            300
        );
    }
    #[test]
    fn one_bad_dictionary_entry_or_sound_value_keeps_everything_else() {
        let saved = br#"{"activation_key":"right_option","speech":{"speed":1.5,"sound":{"volume":9.0},"dictionary":[{"word":"k8s","replacement":"kubernetes","match_case":false},{"word":"K8S","replacement":"duplicate","match_case":false},{"word":"","replacement":"empty","match_case":false}]}}"#;
        let (loaded, reset) = Settings::from_json(saved).unwrap();
        assert_eq!(loaded.speech.speed, 1.5);
        assert_eq!(
            loaded.activation_key,
            crate::activation::KeyPreference::RightOption
        );
        assert_eq!(loaded.speech.sound.volume, 1.);
        assert_eq!(loaded.speech.dictionary.len(), 1);
        assert_eq!(loaded.speech.dictionary[0].replacement, "kubernetes");
        assert_eq!(reset, ["sound settings", "dictionary entries"]);
    }
    #[test]
    fn a_field_with_the_wrong_type_is_reset_alone() {
        let saved = br#"{"check_updates":false,"speech":{"speed":"fast","memory_saver":true,"dictionary":[{"word":"k8s","replacement":"kubernetes","match_case":false},{"word":5}]}}"#;
        let (loaded, reset) = Settings::from_json(saved).unwrap();
        assert!(!loaded.check_updates);
        assert!(loaded.speech.memory_saver);
        assert_eq!(loaded.speech.speed, 1.);
        assert_eq!(loaded.speech.dictionary.len(), 1);
        assert_eq!(reset, ["unreadable settings"]);
    }
}
