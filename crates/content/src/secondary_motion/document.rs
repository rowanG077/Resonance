//! Editable, named dynamics. Joint indices exist only after model preparation.
use super::{Chain, CollisionPlane, Joint};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Definition {
    pub chains: Vec<ChainDefinition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainDefinition {
    pub name: String,
    /// Ordered GLB joint names, including the terminal guide.
    pub joints: Vec<String>,
    pub gravity: Parameter,
    pub damping: Parameter,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub attraction: f32,
    #[serde(
        default = "pose_attraction",
        skip_serializing_if = "default_pose_attraction"
    )]
    pub pose_attraction: f32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub preserve_rotation: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub lock_rotation_x: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub lock_rotation_y: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_plane: Option<PlaneDefinition>,
}

fn pose_attraction() -> f32 {
    0.2
}
fn default_pose_attraction(value: &f32) -> bool {
    *value == pose_attraction()
}
fn full_strength() -> f32 {
    1.
}
fn default_strength(value: &f32) -> bool {
    *value == full_strength()
}

/// One common value, or one value per named joint in the same order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Parameter {
    Uniform(f32),
    PerJoint(Vec<f32>),
}
impl Parameter {
    fn from_values(values: Vec<f32>) -> Self {
        if values.iter().all(|value| *value == values[0]) {
            Self::Uniform(values[0])
        } else {
            Self::PerJoint(values)
        }
    }
    fn values(&self, count: usize) -> Result<Vec<f32>> {
        match self {
            Self::Uniform(value) => Ok(vec![*value; count]),
            Self::PerJoint(values) => {
                ensure!(
                    values.len() == count,
                    "chain parameter needs one value per joint"
                );
                Ok(values.clone())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaneDefinition {
    pub anchor: String,
    pub normal: [f32; 3],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub offset: f32,
    #[serde(default = "full_strength", skip_serializing_if = "default_strength")]
    pub strength: f32,
}

impl Definition {
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }

    pub fn from_chains(chains: Vec<(String, Chain)>, names: &[String]) -> Result<Self> {
        let name = |index: u16| {
            names
                .get(usize::from(index))
                .cloned()
                .context("chain joint is missing")
        };
        let chains = chains
            .into_iter()
            .map(|(label, chain)| {
                chain.validate(names.len())?;
                Ok(ChainDefinition {
                    name: label,
                    joints: chain
                        .joints
                        .iter()
                        .map(|joint| name(joint.node))
                        .collect::<Result<_>>()?,
                    gravity: Parameter::from_values(
                        chain.joints.iter().map(|joint| joint.gravity).collect(),
                    ),
                    damping: Parameter::from_values(
                        chain.joints.iter().map(|joint| joint.damping).collect(),
                    ),
                    attraction: chain.attraction,
                    pose_attraction: pose_attraction(),
                    preserve_rotation: chain.preserve_rotation,
                    lock_rotation_x: chain.rotation_locks[0],
                    lock_rotation_y: chain.rotation_locks[1],
                    collision_plane: chain
                        .collision_plane
                        .map(|plane| {
                            Ok::<_, anyhow::Error>(PlaneDefinition {
                                anchor: name(plane.anchor)?,
                                normal: plane.normal,
                                offset: plane.offset,
                                strength: plane.strength,
                            })
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<_>>()?;
        let definition = Self { chains };
        definition.prepare(names)?;
        Ok(definition)
    }

    pub fn prepare(&self, names: &[String]) -> Result<Vec<Chain>> {
        if self.is_empty() {
            return Ok(vec![]);
        }
        let indices: BTreeMap<_, _> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        ensure!(
            indices.len() == names.len(),
            "chain model has ambiguous joint names"
        );
        let index = |name: &str| -> Result<u16> {
            Ok(u16::try_from(*indices.get(name).with_context(|| {
                format!("chain refers to absent joint {name}")
            })?)?)
        };
        let mut labels = BTreeSet::new();
        self.chains
            .iter()
            .map(|definition| {
                ensure!(
                    !definition.name.trim().is_empty() && labels.insert(&definition.name),
                    "empty or duplicate chain name"
                );
                ensure!(
                    definition.pose_attraction.is_finite()
                        && (0. ..=1.).contains(&definition.pose_attraction),
                    "invalid pose attraction"
                );
                let gravity = definition.gravity.values(definition.joints.len())?;
                let damping = definition.damping.values(definition.joints.len())?;
                let chain = Chain {
                    joints: definition
                        .joints
                        .iter()
                        .zip(gravity)
                        .zip(damping)
                        .map(|((name, gravity), damping)| {
                            Ok(Joint {
                                node: index(name)?,
                                gravity,
                                damping,
                            })
                        })
                        .collect::<Result<_>>()?,
                    attraction: definition.attraction,
                    preserve_rotation: definition.preserve_rotation,
                    rotation_locks: [definition.lock_rotation_x, definition.lock_rotation_y],
                    collision_plane: definition
                        .collision_plane
                        .as_ref()
                        .map(|plane| {
                            Ok::<_, anyhow::Error>(CollisionPlane {
                                anchor: index(&plane.anchor)?,
                                normal: plane.normal,
                                offset: plane.offset,
                                strength: plane.strength,
                            })
                        })
                        .transpose()?,
                };
                chain
                    .validate(names.len())
                    .with_context(|| format!("chain {}", definition.name))?;
                Ok(chain)
            })
            .collect()
    }

    pub fn validate_selection(&self, selected: &[String]) -> Result<()> {
        let mut seen = BTreeSet::new();
        for name in selected {
            ensure!(
                seen.insert(name) && self.chains.iter().any(|chain| &chain.name == name),
                "unknown or duplicate pose chain {name}"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn named_chains_follow_joint_reordering_and_keep_parameter_variants() -> Result<()> {
        let definition: Definition = serde_json::from_value(json!([{
            "name":"cape-left", "joints":["root","middle","tip"],
            "gravity":0.333, "damping":[0.8,0.5,0.2], "attraction":0.03,
            "lock_rotation_y":true,
            "collision_plane":{"anchor":"chest","normal":[0,-1,0]}
        }]))?;
        let names = ["tip", "root", "chest", "middle"].map(String::from);
        let prepared = definition.prepare(&names)?;
        assert_eq!(
            prepared[0]
                .joints
                .iter()
                .map(|j| j.node)
                .collect::<Vec<_>>(),
            [1, 3, 0]
        );
        assert!(prepared[0].joints.iter().all(|j| j.gravity == 0.333));
        assert_eq!(
            prepared[0]
                .joints
                .iter()
                .map(|j| j.damping)
                .collect::<Vec<_>>(),
            [0.8, 0.5, 0.2]
        );
        assert_eq!(prepared[0].collision_plane.as_ref().unwrap().anchor, 2);
        assert_eq!(prepared[0].collision_plane.as_ref().unwrap().strength, 1.);
        assert_eq!(prepared[0].rotation_locks, [false, true]);
        assert_eq!(definition.chains[0].pose_attraction, 0.2);
        assert!(definition.validate_selection(&["cape-left".into()]).is_ok());
        assert!(
            definition
                .validate_selection(&["cape-right".into()])
                .is_err()
        );
        assert!(
            definition
                .validate_selection(&["cape-left".into(), "cape-left".into()])
                .is_err()
        );
        let encoded =
            Definition::from_chains(vec![("cape-left".into(), prepared[0].clone())], &names)?;
        assert_eq!(
            serde_json::to_value(encoded)?,
            serde_json::to_value(definition.clone())?
        );
        let mut renamed = definition.clone();
        renamed.chains[0].joints[0] = "cape-root".into();
        assert!(renamed.prepare(&names).is_err());
        let renamed_names = ["tip", "cape-root", "chest", "middle"].map(String::from);
        assert_eq!(
            serde_json::to_value(renamed.prepare(&renamed_names)?)?,
            serde_json::to_value(&prepared)?
        );
        let mut broken = definition.clone();
        broken.chains[0].damping = Parameter::PerJoint(vec![0.8]);
        assert!(broken.prepare(&names).is_err());
        broken = definition.clone();
        broken.chains.push(broken.chains[0].clone());
        assert!(broken.prepare(&names).is_err());
        assert!(serde_json::from_value::<Definition>(json!({"chains":[]})).is_err());
        assert!(
            serde_json::from_value::<crate::SceneClip>(
                json!({"motion":"pose.glb","resource_slot":12,"secondary_pose_nodes":[0]})
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn model_mods_change_named_dynamics_and_leave_captured_definitions_intact() -> Result<()> {
        use crate::library::{Library, Mod};
        use std::fs;
        let root = tempfile::tempdir()?;
        let edit = tempfile::tempdir()?;
        fs::write(
            root.path().join("content.json"),
            serde_json::to_vec(&crate::library::Descriptor {
                version: crate::library::VERSION,
                game: crate::library::GAME.into(),
            })?,
        )?;
        let model = json!({"parts":[{"resource":0,"mesh":"body.glb","textures":[],"materials":[],
            "secondary_motion":[{"name":"hair","joints":["root","tip"],"gravity":0.4,"damping":0.8}],
            "clips":[{"motion":"pose.glb","slots":[12],"pose_chains":["hair"]}]
        }]});
        fs::write(root.path().join("model.json"), serde_json::to_vec(&model)?)?;
        let library = Library::open(root.path(), &[])?;
        let mut captured = library.snapshot();
        let old: crate::field::document::Model = captured.json("model.json", 4096)?;
        let mut changed = model.clone();
        changed["parts"][0]["secondary_motion"][0]["gravity"] = json!(-0.4);
        changed["parts"][0]["secondary_motion"][0]["pose_attraction"] = json!(0.7);
        fs::write(
            edit.path().join("model.json"),
            serde_json::to_vec(&changed)?,
        )?;
        let modified = Library::open(
            root.path(),
            &[Mod {
                name: "hair-wind".into(),
                root: edit.path().into(),
            }],
        )?;
        let new: crate::field::document::Model = modified.snapshot().json("model.json", 4096)?;
        let names = ["root", "tip"].map(String::from);
        assert_eq!(
            old.parts[0].secondary_motion.prepare(&names)?[0].joints[1].gravity,
            0.4
        );
        assert_eq!(
            new.parts[0].secondary_motion.prepare(&names)?[0].joints[1].gravity,
            -0.4
        );
        assert_eq!(new.parts[0].secondary_motion.chains[0].pose_attraction, 0.7);
        new.parts[0]
            .secondary_motion
            .validate_selection(&new.parts[0].clips[0].pose_chains)?;
        let retained: crate::field::document::Model = captured.json("model.json", 4096)?;
        assert_eq!(
            retained.parts[0].secondary_motion,
            old.parts[0].secondary_motion
        );
        Ok(())
    }
}
