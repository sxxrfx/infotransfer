use crate::assets::SpeechModel;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Female,
    Male,
    Unspecified,
}

/// Kitten names are application aliases; Heart retains its upstream name.
pub struct Voice {
    pub name: &'static str,
    pub info: &'static str,
    pub source_id: &'static str,
    pub gender: Gender,
    pub model: SpeechModel,
    pub speaker: u32,
}

pub const VOICES: [Voice; 14] = [
    Voice {
        name: "Arthur",
        info: "English · Male",
        source_id: "expr-voice-2-m",
        gender: Gender::Male,
        model: SpeechModel::KittenCpu,
        speaker: 0,
    },
    Voice {
        name: "Clara",
        info: "English · Female",
        source_id: "expr-voice-2-f",
        gender: Gender::Female,
        model: SpeechModel::KittenCpu,
        speaker: 1,
    },
    Voice {
        name: "Elliot",
        info: "English · Male",
        source_id: "expr-voice-3-m",
        gender: Gender::Male,
        model: SpeechModel::KittenCpu,
        speaker: 2,
    },
    Voice {
        name: "Maya",
        info: "English · Female",
        source_id: "expr-voice-3-f",
        gender: Gender::Female,
        model: SpeechModel::KittenCpu,
        speaker: 3,
    },
    Voice {
        name: "Theo",
        info: "English · Male",
        source_id: "expr-voice-4-m",
        gender: Gender::Male,
        model: SpeechModel::KittenCpu,
        speaker: 4,
    },
    Voice {
        name: "Nora",
        info: "English · Female",
        source_id: "expr-voice-4-f",
        gender: Gender::Female,
        model: SpeechModel::KittenCpu,
        speaker: 5,
    },
    Voice {
        name: "Owen",
        info: "English · Male",
        source_id: "expr-voice-5-m",
        gender: Gender::Male,
        model: SpeechModel::KittenCpu,
        speaker: 6,
    },
    Voice {
        name: "Ruby",
        info: "English · Female",
        source_id: "expr-voice-5-f",
        gender: Gender::Female,
        model: SpeechModel::KittenCpu,
        speaker: 7,
    },
    Voice {
        name: "Heart",
        info: "American English · Female",
        source_id: "af_heart",
        gender: Gender::Female,
        model: SpeechModel::KokoroHeart,
        speaker: 3,
    },
    Voice {
        name: "Ryan",
        info: "English · Qwen3 · 2.3 GB · Experimental",
        source_id: "Qwen3-TTS-1.7B-CustomVoice-4bit",
        gender: Gender::Male,
        model: SpeechModel::Qwen3,
        speaker: 0,
    },
    Voice {
        name: "VoxCPM2",
        info: "English · Reference voice · Experimental",
        source_id: "VoxCPM2-4bit",
        gender: Gender::Unspecified,
        model: SpeechModel::VoxCpm2,
        speaker: 0,
    },
    Voice {
        name: "CosyVoice3",
        info: "English · Reference voice · Experimental",
        source_id: "Fun-CosyVoice3-0.5B-2512-4bit",
        gender: Gender::Unspecified,
        model: SpeechModel::CosyVoice3,
        speaker: 0,
    },
    Voice {
        name: "Sopro",
        info: "English · Reference voice · Experimental",
        source_id: "sopro-v2-turbo",
        gender: Gender::Unspecified,
        model: SpeechModel::Sopro,
        speaker: 0,
    },
    Voice {
        name: "Phonon",
        info: "English · Default voice · Experimental",
        source_id: "phonon-31m-en-q8",
        gender: Gender::Unspecified,
        model: SpeechModel::Phonon,
        speaker: 0,
    },
];
pub const VOICE_COUNT: u32 = VOICES.len() as u32;
/// Python model voices run a configured program, so they need an opt-in build.
pub fn available(voice: u32) -> bool {
    VOICES.get(voice as usize).is_some_and(|voice| {
        cfg!(feature = "experimental-voices") || voice.model.local_key().is_none()
    })
}
/// Saved by stable source ID, so catalog changes cannot silently change a voice.
pub mod persisted {
    use super::VOICES;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(voice: &u32, serializer: S) -> Result<S::Ok, S::Error> {
        VOICES[*voice as usize].source_id.serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Index(u32),
            Id(String),
        }
        Ok(match Saved::deserialize(deserializer)? {
            Saved::Index(index) => index,
            Saved::Id(id) => VOICES
                .iter()
                .position(|voice| voice.source_id == id)
                .unwrap_or(0) as u32,
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum VoiceFilter {
    #[default]
    All,
    Female,
    Male,
    EmotionInstructions,
    ReferenceStyle,
    NonverbalSounds,
    VoiceDesign,
    JointDialogue,
}
impl VoiceFilter {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::EmotionInstructions => "Emotion",
            Self::ReferenceStyle => "Reference style",
            Self::NonverbalSounds => "Nonverbal",
            Self::JointDialogue => "Dialogue",
            _ => self.label(),
        }
    }
    pub const ALL: &'static [Self] = if cfg!(feature = "experimental-voices") {
        &[
            Self::All,
            Self::Female,
            Self::Male,
            Self::EmotionInstructions,
            Self::ReferenceStyle,
            Self::NonverbalSounds,
            Self::VoiceDesign,
            Self::JointDialogue,
        ]
    } else {
        &[Self::All, Self::Female, Self::Male]
    };
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All voices",
            Self::Female => "Female",
            Self::Male => "Male",
            Self::EmotionInstructions => "Emotion instructions",
            Self::ReferenceStyle => "Reference-style imitation",
            Self::NonverbalSounds => "Nonverbal sounds",
            Self::VoiceDesign => "Voice design",
            Self::JointDialogue => "Joint dialogue generation",
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum VoiceSort {
    #[default]
    Name,
    Fastest,
    Quality,
}
impl VoiceSort {
    pub const ALL: [Self; 3] = [Self::Name, Self::Fastest, Self::Quality];
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Fastest => "Fastest",
            Self::Quality => "Quality",
        }
    }
}
#[derive(serde::Deserialize)]
pub struct Rating {
    pub voice: u32,
    pub startup_p95_ms: Option<f64>,
    pub naturalness_median: Option<f32>,
}
#[derive(serde::Deserialize)]
pub struct Ratings {
    pub detail: String,
    pub quality_detail: String,
    pub voices: Vec<Rating>,
}
pub static RATINGS: std::sync::LazyLock<Ratings> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../resources/voice-ratings.json"))
        .expect("embedded voice ratings")
});
pub fn rating(voice: u32) -> Option<&'static Rating> {
    RATINGS.voices.iter().find(|rating| rating.voice == voice)
}

pub fn visible_voices(
    filter: VoiceFilter,
    installed_only: bool,
    sort: VoiceSort,
    ratings: &[Rating],
    installed: impl Fn(u32) -> bool,
) -> Vec<u32> {
    let mut ids: Vec<_> = VOICES
        .iter()
        .enumerate()
        .filter_map(|(id, voice)| {
            let matches = match filter {
                VoiceFilter::All => true,
                VoiceFilter::Female => voice.gender == Gender::Female,
                VoiceFilter::Male => voice.gender == Gender::Male,
                VoiceFilter::EmotionInstructions => matches!(
                    voice.model,
                    SpeechModel::Qwen3 | SpeechModel::VoxCpm2 | SpeechModel::CosyVoice3
                ),
                VoiceFilter::ReferenceStyle => matches!(
                    voice.model,
                    SpeechModel::VoxCpm2 | SpeechModel::CosyVoice3 | SpeechModel::Sopro
                ),
                VoiceFilter::NonverbalSounds => voice.model == SpeechModel::CosyVoice3,
                VoiceFilter::VoiceDesign => voice.model == SpeechModel::VoxCpm2,
                VoiceFilter::JointDialogue => false,
            };
            (matches && available(id as u32) && (!installed_only || installed(id as u32)))
                .then_some(id as u32)
        })
        .collect();
    let measure = |id| ratings.iter().find(|rating| rating.voice == id);
    ids.sort_by(|a, b| {
        let order = match sort {
            VoiceSort::Name => std::cmp::Ordering::Equal,
            VoiceSort::Fastest => {
                let a = measure(*a).and_then(|rating| rating.startup_p95_ms);
                let b = measure(*b).and_then(|rating| rating.startup_p95_ms);
                match (a, b) {
                    (Some(a), Some(b)) => a.total_cmp(&b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            }
            VoiceSort::Quality => {
                let a = measure(*a).and_then(|rating| rating.naturalness_median);
                let b = measure(*b).and_then(|rating| rating.naturalness_median);
                match (a, b) {
                    (Some(a), Some(b)) => b.total_cmp(&a),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            }
        };
        order.then_with(|| VOICES[*a as usize].name.cmp(VOICES[*b as usize].name))
    });
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "experimental-voices")]
    #[test]
    fn capability_filters_use_the_selected_checkpoint() {
        let filtered = |filter| visible_voices(filter, false, VoiceSort::Name, &[], |_| true);
        assert_eq!(filtered(VoiceFilter::EmotionInstructions), [11, 9, 10]);
        assert_eq!(filtered(VoiceFilter::ReferenceStyle), [11, 12, 10]);
        assert_eq!(filtered(VoiceFilter::NonverbalSounds), [11]);
        assert_eq!(filtered(VoiceFilter::VoiceDesign), [10]);
        assert!(filtered(VoiceFilter::JointDialogue).is_empty());
        assert!(!filtered(VoiceFilter::VoiceDesign).contains(&9));
    }
    #[test]
    fn voices_are_saved_by_stable_id_and_old_indexes_still_load() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Saved(#[serde(with = "persisted")] u32);
        assert_eq!(serde_json::to_string(&Saved(8)).unwrap(), "\"af_heart\"");
        assert_eq!(serde_json::from_str::<Saved>("\"af_heart\"").unwrap().0, 8);
        assert_eq!(serde_json::from_str::<Saved>("3").unwrap().0, 3);
        assert_eq!(serde_json::from_str::<Saved>("\"gone\"").unwrap().0, 0);
    }
    #[test]
    fn filtering_and_sorting_keep_catalog_identity_and_unknowns_last() {
        let ratings = [
            Rating {
                voice: 3,
                startup_p95_ms: Some(110.),
                naturalness_median: Some(3.2),
            },
            Rating {
                voice: 1,
                startup_p95_ms: Some(223.),
                naturalness_median: Some(4.1),
            },
        ];
        let ids = visible_voices(
            VoiceFilter::Female,
            true,
            VoiceSort::Fastest,
            &ratings,
            |id| id < 8,
        );
        assert_eq!(ids, [3, 1, 5, 7]);
        assert_eq!(VOICES[ids[0] as usize].name, "Maya");
        assert_eq!(
            visible_voices(
                VoiceFilter::Female,
                true,
                VoiceSort::Quality,
                &ratings,
                |id| id < 8
            ),
            [1, 3, 5, 7]
        );
        assert!(
            visible_voices(VoiceFilter::Male, true, VoiceSort::Name, &[], |_| false).is_empty()
        );
    }
}
