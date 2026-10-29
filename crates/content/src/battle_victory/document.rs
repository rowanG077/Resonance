//! Victory clips belong to a selected character/costume, independently of the
//! bytes of its model. Group selection keeps its original gameplay identities.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::Context;
use std::collections::BTreeMap;

mod groups;
pub use groups::{NamedGroup, NamedSelection, RandomBits};
pub const SELECTORS: [&str; 5] = [
    "healthy-party",
    "injured-leader",
    "ordinary",
    "rare",
    "flawless",
];
pub fn selector_name(id: u8) -> Result<&'static str> {
    SELECTORS
        .get(usize::from(id))
        .copied()
        .context("unknown victory selector")
}
pub const GROUPS: &str = "battle/victory/groups.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub file: String,
    pub costume: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition<C = BTreeMap<String, Performance>, T = Timeline> {
    pub costumes: BTreeMap<String, C>,
    pub timelines: BTreeMap<String, T>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Performance {
    pub motion: String,
    pub timeline: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<(u16, Event)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

pub fn capture<'a>(
    snapshot: &mut Snapshot,
    selections: impl IntoIterator<Item = (u8, u8, &'a Selection)>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Performances> {
    ensure!(!cancelled(), "victory preparation cancelled");
    let groups = snapshot.json::<Vec<serde_json::Value>>(GROUPS, JSON_LIMIT);
    let mut result: Performances = Performances::default();
    if let Some(groups) = diagnostics.attempt(GROUPS, groups)? {
        let mut names: Option<crate::library::Mappings> = None;
        for (index, row) in groups.into_iter().enumerate() {
            ensure!(!cancelled(), "victory preparation cancelled");
            if let Some(group) = diagnostics.attempt(
                &format!("{GROUPS} row {index}"),
                serde_json::from_value::<NamedGroup>(row)
                    .map_err(anyhow::Error::from)
                    .and_then(|row| {
                        if names.is_none() {
                            names = Some(
                                snapshot.json(crate::battle_model::document::PARTY, JSON_LIMIT)?,
                            );
                        }
                        row.prepare(names.as_ref().unwrap())
                    }),
            )? {
                result.groups.push(group);
            }
        }
    }
    let mut selected = std::collections::BTreeSet::new();
    for (character, costume, selection) in selections {
        let path = &selection.file;
        ensure!(!cancelled(), "victory preparation cancelled");
        ensure!(
            (1..=9).contains(&character) && selected.insert((character, costume)),
            "invalid or duplicate victory owner"
        );
        // Decode only the selected costume and each selected timeline. A broken
        // unused costume must not suppress another costume's celebrations.
        let definition =
            snapshot.json::<Definition<serde_json::Value, serde_json::Value>>(path, JSON_LIMIT);
        let Some(mut definition) = diagnostics.attempt(path, definition)? else {
            continue;
        };
        let performances = (|| -> Result<BTreeMap<String, serde_json::Value>> {
            let value = definition
                .costumes
                .remove(&selection.costume)
                .with_context(|| format!("missing victory costume {}", selection.costume))?;
            Ok(serde_json::from_value(value)?)
        })();
        let owner = format!("{path} costume {}", selection.costume);
        let Some(performances) = diagnostics.attempt(&owner, performances)? else {
            continue;
        };
        let mut performances: Vec<_> = performances.into_iter().collect();
        performances.sort_by_key(|(key, _)| {
            SELECTORS
                .iter()
                .position(|name| name == key)
                .unwrap_or(usize::MAX)
        });
        for (key, value) in performances {
            ensure!(!cancelled(), "victory preparation cancelled");
            let captured = (|| {
                let selector = SELECTORS
                    .iter()
                    .position(|&name| name == key)
                    .with_context(|| format!("unknown victory selector {key} in {path}"))?
                    as u8;
                let performance: Performance = serde_json::from_value(value)?;
                let timeline: Timeline = serde_json::from_value(
                    definition
                        .timelines
                        .get(&performance.timeline)
                        .with_context(|| {
                            format!("missing victory timeline {}", performance.timeline)
                        })?
                        .clone(),
                )?;
                snapshot.read(&performance.motion, ASSET_LIMIT)?;
                Ok(Ordinary {
                    character,
                    costume: Some(costume),
                    selector,
                    motion: performance.motion,
                    timeline: timeline.events,
                    unsupported_reason: timeline.unsupported_reason,
                })
            })();
            if let Some(row) =
                diagnostics.attempt(&format!("{owner} performance {key}"), captured)?
            {
                result.ordinary.push(row);
            }
        }
    }
    ensure!(!cancelled(), "victory preparation cancelled");
    Ok(result)
}
