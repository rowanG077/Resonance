//! One file owns its named presets; references never cross files or inherit.
use super::*;
use crate::field_audio::reverb::{Buses, Parameters};
use anyhow::{Context, ensure};
use std::collections::{BTreeMap, BTreeSet};

const KEEP_CURRENT: &str = "keep_current";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Document {
    reverbs: BTreeMap<String, Parameters>,
    field_reverbs: Buses<String>,
    battle_reverbs: Buses<String>,
    /// Named reverb -> music IDs. keep_current deliberately leaves bus A alone.
    music_reverbs: BTreeMap<String, Vec<u16>>,
    voice_gains: Vec<f32>,
}

impl From<Mix> for Document {
    fn from(mix: Mix) -> Self {
        let mut reverbs = BTreeMap::<String, Parameters>::new();
        let mut name = |preferred: String, values: [f32; 5]| {
            let values = Parameters::from(values);
            if let Some((name, _)) = reverbs.iter().find(|(_, previous)| **previous == values) {
                return name.clone();
            }
            reverbs.insert(preferred.clone(), values);
            preferred
        };
        let field_reverbs = Buses {
            auxiliary_a: name("field".into(), mix.field_reverbs[0]),
            auxiliary_b: name("effects".into(), mix.field_reverbs[1]),
        };
        let battle_reverbs = Buses {
            auxiliary_a: name("battle".into(), mix.battle_reverbs[0]),
            auxiliary_b: name("battle-effects".into(), mix.battle_reverbs[1]),
        };
        let presets = mix
            .music_reverbs
            .presets
            .iter()
            .enumerate()
            .map(|(index, &parameters)| name(format!("music-{}", index + 1), parameters))
            .collect::<Vec<_>>();
        let mut music_reverbs = BTreeMap::<String, Vec<u16>>::new();
        // Keep a deliberate unselected preset editable, without repeating values.
        for preset in &presets {
            music_reverbs.entry(preset.clone()).or_default();
        }
        for (id, selector) in mix.music_reverbs.selectors.iter().copied().enumerate() {
            let preset = if selector == 0 {
                KEEP_CURRENT
            } else {
                &presets[usize::from(selector - 1)]
            };
            music_reverbs
                .entry(preset.into())
                .or_default()
                .push(id as u16);
        }
        Self {
            reverbs,
            field_reverbs,
            battle_reverbs,
            music_reverbs,
            voice_gains: mix.voice_gains,
        }
    }
}

impl TryFrom<Document> for Mix {
    type Error = anyhow::Error;
    fn try_from(doc: Document) -> Result<Self> {
        ensure!(
            !doc.reverbs.contains_key(KEEP_CURRENT),
            "keep_current is a music-selection instruction, not a reverb name"
        );
        ensure!(
            doc.reverbs.keys().all(|n| !n.is_empty()),
            "empty reverb name"
        );
        let parameters = |name: &str| -> Result<[f32; 5]> {
            let value = doc
                .reverbs
                .get(name)
                .with_context(|| format!("unknown reverb {name}"))?;
            MusicReverbs::validate_preset((*value).into())
        };
        let buses = |buses: &Buses<String>| -> Result<[[f32; 5]; 2]> {
            Ok([
                parameters(&buses.auxiliary_a)?,
                parameters(&buses.auxiliary_b)?,
            ])
        };
        let field_reverbs = buses(&doc.field_reverbs)?;
        let battle_reverbs = buses(&doc.battle_reverbs)?;
        let mut names = Vec::new();
        // Stable context order also matches the ordinary imported catalogue.
        for name in [
            &doc.field_reverbs.auxiliary_a,
            &doc.battle_reverbs.auxiliary_a,
        ]
        .into_iter()
        .chain(doc.music_reverbs.keys())
        {
            if name != KEEP_CURRENT && doc.music_reverbs.contains_key(name) && !names.contains(name)
            {
                names.push(name.clone());
            }
        }
        ensure!(
            names.len() <= usize::from(u8::MAX),
            "too many music reverb choices"
        );
        let presets = names
            .iter()
            .map(|name| parameters(name))
            .collect::<Result<Vec<_>>>()?;
        let last = doc.music_reverbs.values().flatten().copied().max();
        ensure!(
            last.is_none_or(|id| id <= i16::MAX as u16),
            "music ID exceeds the audio request range"
        );
        let mut selectors = vec![0; last.map_or(0, |id| usize::from(id) + 1)];
        let mut assigned = BTreeSet::new();
        for (name, ids) in doc.music_reverbs {
            let selector = if name == KEEP_CURRENT {
                0
            } else {
                u8::try_from(
                    names
                        .iter()
                        .position(|n| *n == name)
                        .context("unresolved music reverb")?
                        + 1,
                )?
            };
            for id in ids {
                ensure!(
                    assigned.insert(id),
                    "music {id} selects more than one reverb action"
                );
                selectors[usize::from(id)] = selector;
            }
        }
        ensure!(
            assigned.len() == selectors.len(),
            "music reverb choices must include every ID through their last selection; use keep_current to preserve the bus"
        );
        let mix = Self {
            music_reverbs: MusicReverbs { presets, selectors },
            voice_gains: doc.voice_gains,
            field_reverbs,
            battle_reverbs,
        };
        mix.validate()?;
        Ok(mix)
    }
}
