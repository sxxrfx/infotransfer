use crate::assets::SpeechModel;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Female,
    Male,
    Unspecified,
}

/// Paradee v1.2 voices, distilled from the Kokoro presets of the same names.
pub struct Voice {
    pub name: &'static str,
    pub info: &'static str,
    pub source_id: &'static str,
    pub gender: Gender,
    pub model: SpeechModel,
    pub speaker: u32,
}

/// Isabella (speaker 1) was removed after listening: robotic through the app.
pub const VOICES: [Voice; 1] = [Voice {
    name: "Heart",
    info: "American English · Female",
    source_id: "af_heart",
    gender: Gender::Female,
    model: SpeechModel::Paradee,
    speaker: 0,
}];
pub const VOICE_COUNT: u32 = VOICES.len() as u32;
pub fn available(voice: u32) -> bool {
    voice < VOICE_COUNT
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
            // Older settings saved catalog indexes, which may name a removed voice.
            Saved::Index(index) if index < super::VOICE_COUNT => index,
            Saved::Index(_) => 0,
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
}
impl VoiceFilter {
    pub fn short_label(self) -> &'static str {
        self.label()
    }
    pub const ALL: &'static [Self] = &[Self::All, Self::Female, Self::Male];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All voices",
            Self::Female => "Female",
            Self::Male => "Male",
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
    #[test]
    fn voices_are_saved_by_stable_id_and_removed_voices_fall_back_to_heart() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Saved(#[serde(with = "persisted")] u32);
        assert_eq!(serde_json::to_string(&Saved(0)).unwrap(), "\"af_heart\"");
        assert_eq!(serde_json::from_str::<Saved>("\"af_heart\"").unwrap().0, 0);
        assert_eq!(
            serde_json::from_str::<Saved>("\"expr-voice-2-m\"")
                .unwrap()
                .0,
            0
        );
        assert_eq!(serde_json::from_str::<Saved>("8").unwrap().0, 0);
        assert_eq!(
            serde_json::from_str::<Saved>("\"bf_isabella\"").unwrap().0,
            0
        );
        assert!(
            VOICES
                .iter()
                .all(|voice| voice.speaker < voice.model.speakers())
        );
    }
    #[test]
    fn filtering_and_sorting_keep_catalog_identity_and_unknowns_last() {
        let ratings = [Rating {
            voice: 0,
            startup_p95_ms: Some(110.),
            naturalness_median: Some(3.2),
        }];
        for sort in VoiceSort::ALL {
            assert_eq!(
                visible_voices(VoiceFilter::Female, true, sort, &ratings, |_| true),
                [0]
            );
        }
        assert!(
            visible_voices(VoiceFilter::Male, false, VoiceSort::Name, &[], |_| true).is_empty()
        );
        assert!(visible_voices(VoiceFilter::All, true, VoiceSort::Name, &[], |_| false).is_empty());
    }
}
