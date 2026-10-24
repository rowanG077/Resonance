//! Victory clips belong to a selected character/costume, independently of the
//! bytes of its model. Group selection keeps its original gameplay identities.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use std::collections::BTreeMap;

pub const VERSION: u32 = 1;
pub const GROUPS: &str = "battle/victory/groups.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition<P = BTreeMap<u8, Performance>> {
    pub version: u32,
    pub performances: P,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Performance {
    pub motion: String,
    pub timeline: Option<Vec<(u16, Event)>>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Groups<G = Vec<Group>> {
    pub version: u32,
    pub groups: G,
}

pub fn capture<'a>(
    snapshot: &mut Snapshot,
    selections: impl IntoIterator<Item = (u8, u8, &'a str)>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Performances> {
    ensure!(!cancelled(), "victory preparation cancelled");
    let groups = snapshot
        .json::<Groups<Vec<serde_json::Value>>>(GROUPS, JSON_LIMIT)
        .and_then(|groups| {
            ensure!(
                groups.version == VERSION,
                "unsupported victory group version"
            );
            Ok(groups)
        });
    let mut result: Performances = Performances::default();
    if let Some(groups) = diagnostics.attempt(GROUPS, groups)? {
        for (index, row) in groups.groups.into_iter().enumerate() {
            ensure!(!cancelled(), "victory preparation cancelled");
            if let Some(group) = diagnostics.attempt(
                &format!("{GROUPS} row {index}"),
                serde_json::from_value(row).map_err(Into::into),
            )? {
                result.groups.push(group);
            }
        }
    }
    let mut selected = std::collections::BTreeSet::new();
    for (character, costume, path) in selections {
        ensure!(!cancelled(), "victory preparation cancelled");
        ensure!(
            (1..=9).contains(&character) && selected.insert((character, costume)),
            "invalid or duplicate victory owner"
        );
        let definition = snapshot
            .json::<Definition<BTreeMap<String, serde_json::Value>>>(path, JSON_LIMIT)
            .and_then(|definition| {
                ensure!(
                    definition.version == VERSION,
                    "unsupported victory version in {path}"
                );
                Ok(definition)
            });
        let Some(definition) = diagnostics.attempt(path, definition)? else {
            continue;
        };
        for (key, value) in definition.performances {
            ensure!(!cancelled(), "victory preparation cancelled");
            let captured = (|| {
                let selector = key.parse::<u8>()?;
                ensure!(
                    selector < 5 && selector.to_string() == key,
                    "unknown victory selector {selector} in {path}"
                );
                let performance: Performance = serde_json::from_value(value)?;
                snapshot.read(&performance.motion, ASSET_LIMIT)?;
                Ok(Ordinary {
                    character,
                    costume: Some(costume),
                    selector,
                    source_sha256: String::new(),
                    body_sha256: String::new(),
                    motion: performance.motion,
                    timeline: performance.timeline,
                    unsupported_reason: performance.unsupported_reason,
                })
            })();
            if let Some(row) =
                diagnostics.attempt(&format!("{path} performance {key}"), captured)?
            {
                result.ordinary.push(row);
            }
        }
    }
    ensure!(!cancelled(), "victory preparation cancelled");
    Ok(result)
}
