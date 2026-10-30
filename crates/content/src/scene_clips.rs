//! A performance can serve several slots and banks without repeating its path.
//! Prepared clips remain individual requests for gameplay and rendering.
use crate::SceneClip;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    motion: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    joints: BTreeMap<String, Option<String>>,
    slots: Vec<u16>,
    /// An omitted bank list binds to the model itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    banks: Vec<u32>,
    #[serde(default, skip_serializing_if = "SceneClip::duration_is_derived")]
    duration_seconds: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pose_chains: Vec<String>,
}
impl Binding {
    fn same_performance(&self, clip: &SceneClip) -> bool {
        self.motion == clip.motion
            && self.joints == clip.joints
            && self.duration_seconds == clip.duration_seconds
            && self.pose_chains == clip.pose_chains
    }
    fn same_bank(&self, bank: Option<u32>) -> bool {
        match bank {
            Some(bank) => self.banks == [bank],
            None => self.banks.is_empty(),
        }
    }
}

pub fn serialize<S: Serializer>(clips: &[SceneClip], serializer: S) -> Result<S::Ok, S::Error> {
    let mut groups = Vec::<Binding>::new();
    let mut first_slot = BTreeMap::<u16, usize>::new();
    for clip in clips {
        // Some consumers choose the first matching slot without a bank. Do not
        // move a later alias ahead of an earlier, different performance.
        let group = groups.iter().enumerate().position(|(index, group)| {
            group.same_performance(clip)
                && group.same_bank(clip.animation_resource)
                && first_slot
                    .get(&clip.resource_slot)
                    .is_none_or(|&first| index >= first)
        });
        let index = if let Some(index) = group {
            groups[index].slots.push(clip.resource_slot);
            index
        } else {
            groups.push(Binding {
                motion: clip.motion.clone(),
                joints: clip.joints.clone(),
                slots: vec![clip.resource_slot],
                banks: clip.animation_resource.into_iter().collect(),
                duration_seconds: clip.duration_seconds,
                pose_chains: clip.pose_chains.clone(),
            });
            groups.len() - 1
        };
        first_slot.entry(clip.resource_slot).or_insert(index);
    }
    // Only equal slot lists share banks: never invent a Cartesian-product target.
    let mut result = Vec::<Binding>::new();
    for group in groups {
        let existing = result.iter_mut().find(|old| {
            !old.banks.is_empty()
                && !group.banks.is_empty()
                && old.slots == group.slots
                && old.motion == group.motion
                && old.joints == group.joints
                && old.duration_seconds == group.duration_seconds
                && old.pose_chains == group.pose_chains
        });
        if let Some(existing) = existing {
            existing.banks.extend(group.banks);
        } else {
            result.push(group);
        }
    }
    result.serialize(serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<SceneClip>, D::Error> {
    let groups = Vec::<Binding>::deserialize(deserializer)?;
    let mut clips = Vec::new();
    let mut requests = BTreeSet::new();
    for group in groups {
        if group.slots.is_empty() {
            return Err(D::Error::custom("animation binding has no slots"));
        }
        crate::validate_asset_path(&group.motion).map_err(D::Error::custom)?;
        let banks = if group.banks.is_empty() {
            vec![None]
        } else {
            group.banks.into_iter().map(Some).collect()
        };
        for bank in banks {
            for &slot in &group.slots {
                if !requests.insert((bank, slot)) {
                    return Err(D::Error::custom(format!(
                        "duplicate animation request {bank:?}/{slot}"
                    )));
                }
                clips.push(SceneClip {
                    motion: group.motion.clone(),
                    joints: group.joints.clone(),
                    resource_slot: slot,
                    animation_resource: bank,
                    duration_seconds: group.duration_seconds,
                    pose_chains: group.pose_chains.clone(),
                });
            }
        }
    }
    Ok(clips)
}

#[cfg(test)]
mod tests;
