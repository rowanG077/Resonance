//! Event-owned scripts and actor bindings, captured before combat is suspended.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Result, ensure};

pub const VERSION: u32 = 1;
pub const MAPPINGS: &str = "battle/events/mappings.json";
pub const BLINK: &str = "battle/events/blink.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub script: String,
    pub messages: String,
    pub blink: String,
    pub audio: crate::battle_audio::Selection,
    pub actors: BTreeMap<u32, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub version: u32,
    pub skeleton: Skeleton,
    pub parts: Vec<ScenePart>,
}

pub fn capture(
    snapshot: &mut Snapshot,
    id: u16,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Scene> {
    ensure!(!cancelled(), "battle event preparation cancelled");
    let path = snapshot.resolve(MAPPINGS, u32::from(id), Some("event.json"))?;
    let definition: Definition = snapshot.json(&path, JSON_LIMIT)?;
    ensure!(
        definition.version == VERSION,
        "unsupported battle event in {path}"
    );
    snapshot.read(&definition.script, ASSET_LIMIT)?;
    snapshot.read(&definition.messages, JSON_LIMIT)?;
    let blink: crate::effect::BlinkCycle = snapshot.json(&definition.blink, JSON_LIMIT)?;
    blink.validate()?;
    let mut actors = BTreeMap::new();
    for (resource, path) in definition.actors {
        ensure!(!cancelled(), "battle event preparation cancelled");
        let mut model: Actor = snapshot.json(&path, JSON_LIMIT)?;
        ensure!(
            model.version == VERSION && !model.parts.is_empty(),
            "invalid event actor in {path}"
        );
        model.skeleton.validate()?;
        ensure!(
            model
                .skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(model.parts[0].bone_names.iter()),
            "battle event rig differs from body in {path}"
        );
        for part in &mut model.parts {
            crate::battle_model::document::capture_scene(
                snapshot,
                &path,
                part,
                diagnostics,
                cancelled,
            )?;
        }
        actors.insert(
            resource,
            Model {
                skeleton: model.skeleton,
                parts: model.parts,
            },
        );
    }
    ensure!(!cancelled(), "battle event preparation cancelled");
    Ok(Scene {
        script: definition.script,
        messages: definition.messages,
        blink: Some(blink),
        audio: definition.audio,
        actors,
    })
}
