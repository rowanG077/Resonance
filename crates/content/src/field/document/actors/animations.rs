//! Animation names select IDs; explicit paths share media with other owners.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum AnimationTarget {
    Named(String),
    Shared { name: String, motion: String },
    Alias { name: String, resource: u32 },
}

impl AnimationTarget {
    pub fn name(&self) -> &str {
        match self {
            Self::Named(name) | Self::Shared { name, .. } | Self::Alias { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct AnimationMappings(pub BTreeMap<u32, AnimationTarget>);

impl<'de> Deserialize<'de> for AnimationMappings {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Ok(Self(
            crate::library::MappingValues::<AnimationTarget>::deserialize(deserializer)?.0,
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationJoint {
    pub id: u16,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub animated: bool,
}

impl AnimationMappings {
    pub fn iter(&self) -> impl Iterator<Item = (u32, &str)> {
        self.0.iter().map(|(&id, target)| (id, target.name()))
    }

    pub fn target(&self, id: u32) -> Result<&str> {
        self.0
            .get(&id)
            .map(AnimationTarget::name)
            .context("unmapped animation resource")
    }

    pub fn motion(&self, id: u32) -> Result<String> {
        let target = &self.0[&self.resource(id)?];
        validate_path(target.name())?;
        let path = match target {
            AnimationTarget::Named(name) => format!("animations/{name}.glb"),
            AnimationTarget::Shared { motion, .. } => motion.clone(),
            AnimationTarget::Alias { .. } => unreachable!(),
        };
        validate_path(&path)?;
        Ok(path)
    }

    pub fn resource(&self, mut id: u32) -> Result<u32> {
        let mut visited = BTreeSet::new();
        loop {
            ensure!(visited.insert(id), "animation alias cycle at {id}");
            match self.0.get(&id).context("unmapped animation resource")? {
                AnimationTarget::Alias { resource, .. } => id = *resource,
                _ => return Ok(id),
            }
        }
    }
}

impl SharedBankBindings {
    /// Dynamic banks use the same name-first, ID-fallback rule as the native
    /// loader. Store each rig and bank once rather than their cross product.
    pub(super) fn fallback(
        &self,
        resource: u32,
        motions: &AnimationMappings,
        models: &ModelMappings,
        actors: &mut [ActorAssets],
    ) -> Result<bool> {
        let Some(tracks) = self.tracks.get(&motions.resource(resource)?) else {
            return Ok(false);
        };
        let mut applied = false;
        for actor in actors {
            let model = models.0.get(&actor.resource);
            for (part_index, target) in actor.parts.iter_mut().enumerate() {
                let Some(rig) = self.rigs.get(&target.mesh) else {
                    continue;
                };
                if self.banks.get(&resource).is_some_and(|rows| {
                    rows.iter().any(|row| {
                        model
                            .is_some_and(|model| row.actors.iter().any(|name| name == model.name()))
                            && row.parts.contains(&(part_index as u16))
                    })
                }) {
                    continue;
                }
                let by_name =
                    tracks
                        .iter()
                        .filter_map(|track| track.source.as_ref())
                        .any(|source| {
                            rig.iter()
                                .any(|joint| joint.source.as_ref() == Some(source))
                        });
                let selected = rig
                    .iter()
                    .map(|joint| {
                        tracks.iter().find(|track| {
                            if by_name {
                                track.source.is_some() && track.source == joint.source
                            } else {
                                track.id == joint.id
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                let joints = rig
                    .iter()
                    .zip(&selected)
                    .map(|(joint, track)| {
                        (joint.name.clone(), track.map(|track| track.name.clone()))
                    })
                    .collect();
                let animated = rig
                    .iter()
                    .zip(&selected)
                    .filter(|(_, track)| track.is_some_and(|track| track.animated))
                    .map(|(joint, _)| joint.name.as_str())
                    .collect::<BTreeSet<_>>();
                let pose_chains = target
                    .secondary_motion
                    .chains
                    .iter()
                    .filter(|chain| {
                        chain
                            .joints
                            .first()
                            .is_some_and(|joint| animated.contains(joint.as_str()))
                    })
                    .map(|chain| chain.name.clone())
                    .collect();
                let clip = SceneClip {
                    motion: motions.motion(resource)?,
                    joints,
                    resource_slot: 12,
                    pose_chains,
                    animation_resource: Some(resource),
                    duration_seconds: 0.,
                };
                if let Some(previous) = target.clips.iter_mut().find(|clip| {
                    clip.animation_resource == Some(resource) && clip.resource_slot == 12
                }) {
                    *previous = clip;
                } else {
                    target.clips.push(clip);
                }
                applied = true;
            }
        }
        Ok(applied)
    }
}
