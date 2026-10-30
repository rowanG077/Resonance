//! Event-owned scripts and actor bindings, captured before combat is suspended.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Result, ensure};

pub const MAPPINGS: &str = "battle/events/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blink: Option<crate::effect::BlinkCycle>,
    pub audio: crate::battle_audio::Selection,
    pub actors: BTreeMap<u32, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub source: ActorSource,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::scene_clips"
    )]
    pub clips: Vec<crate::SceneClip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<crate::AppearanceTextures>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActorSource {
    Party {
        path: String,
    },
    Model {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body_parts: Option<usize>,
    },
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
    snapshot.read(&definition.script, ASSET_LIMIT)?;
    if let Some(path) = &definition.messages {
        snapshot.read(path, JSON_LIMIT)?;
    }
    let settings: crate::field::settings::Settings =
        snapshot.json(crate::field::settings::PATH, JSON_LIMIT)?;
    let blink = definition.blink.unwrap_or(settings.blink);
    blink.validate()?;
    let mut actors = BTreeMap::new();
    for (resource, path) in definition.actors {
        ensure!(!cancelled(), "battle event preparation cancelled");
        let actor: Actor = snapshot.json(&path, JSON_LIMIT)?;
        let mut parts = match &actor.source {
            ActorSource::Party { path } => {
                let mut party: crate::battle_model::document::PartyDefinition =
                    snapshot.json(path, JSON_LIMIT)?;
                party.take_model_parts()?
            }
            ActorSource::Model { path, body_parts } => {
                let parts: Vec<_> = crate::model_preview::document::read(path, |resource| {
                    snapshot.read(resource, JSON_LIMIT)
                })?
                .parts
                .into_iter()
                .map(|part| part.scene)
                .collect();
                let count = body_parts.unwrap_or(parts.len());
                ensure!(
                    count > 0 && count <= parts.len(),
                    "invalid event body part count in {path}"
                );
                parts.into_iter().take(count).collect()
            }
        };
        ensure!(!parts.is_empty(), "invalid event actor in {path}");
        for (index, part) in parts.iter_mut().enumerate() {
            let original = std::mem::take(&mut part.clips);
            part.clips = match &actor.source {
                ActorSource::Party { .. } => {
                    let mut clips = actor.clips.clone();
                    clips.extend(original);
                    clips
                }
                ActorSource::Model { .. } => {
                    let mut clips = original;
                    clips.extend(actor.clips.clone());
                    clips
                }
            };
            if index == 0
                && let Some(appearance) = &actor.appearance
            {
                part.appearance = Some(appearance.clone());
            }
            crate::battle_model::document::capture_scene(
                snapshot,
                &path,
                part,
                diagnostics,
                cancelled,
            )?;
        }
        let skeleton = Skeleton::from_glb(&snapshot.read(&parts[0].mesh, ASSET_LIMIT)?)?;
        actors.insert(resource, Model { skeleton, parts });
    }
    ensure!(!cancelled(), "battle event preparation cancelled");
    Ok(Scene {
        joint_mappings: snapshot.json(crate::field::document::JOINT_MAPPINGS, JSON_LIMIT)?,
        script: definition.script,
        messages: definition.messages,
        blink: Some(blink),
        audio: definition.audio,
        actors,
    })
}
