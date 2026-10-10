//! Prepare immutable battle artwork for scene-owned playback.
#[cfg(test)]
mod body_tests;
use anyhow::{Context, Result, ensure};
use resonance_battle::{Actor, ModelDefinition, Playback, ReactionMotions, Side};
use resonance_content::{animation::Motion, battle_model, prepared::Files};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    Party(u8),
    Enemy(u8),
    /// Equipment or a battle-only weapon resource ID.
    Weapon(u16),
    /// Independent models and textures owned by a stored spell scene.
    Scene(u16),
}

/// Load selected model resources into an owned candidate. The descriptors
/// themselves must already belong to the verified snapshot.
pub fn load_files(
    root: &Path,
    files: Files,
    models: &[ModelSource],
    cache: &mut resonance_content::prepared::Cache,
    cancelled: impl Fn() -> bool,
) -> Result<Files> {
    let mut inventory = BTreeMap::new();
    for &source in models {
        let dependencies = match source {
            ModelSource::Party(character) => {
                files
                    .json::<battle_model::Party>(&battle_model::party_path(character))?
                    .files
            }
            ModelSource::Enemy(id) => {
                files
                    .json::<battle_model::Enemy>(&battle_model::enemy_path(id))?
                    .files
            }
            ModelSource::Weapon(item) => weapon(&files, item)?.files,
            ModelSource::Scene(technique) => {
                files
                    .json::<resonance_content::battle_scene::Scene>(
                        &resonance_content::battle_scene::path(technique),
                    )?
                    .files
            }
        };
        merge_dependencies(&mut inventory, dependencies)?;
    }
    files.with_dependencies(root, inventory, cache, cancelled)
}

/// Models that share a path must agree on its bytes and retain all its uses.
pub(crate) fn merge_dependencies(
    inventory: &mut BTreeMap<String, resonance_content::field_preload::File>,
    dependencies: impl IntoIterator<Item = (String, resonance_content::field_preload::File)>,
) -> Result<()> {
    for (path, file) in dependencies {
        if let Some(previous) = inventory.get_mut(&path) {
            ensure!(
                previous.sha256 == file.sha256 && previous.bytes == file.bytes,
                "inconsistent model dependency {path}"
            );
            previous.roles.extend(file.roles);
        } else {
            inventory.insert(path, file);
        }
    }
    Ok(())
}

#[cfg(test)]
mod dependency_tests {
    use super::merge_dependencies;
    use resonance_content::field_preload::{File, Role};
    use std::collections::BTreeMap;

    fn file(role: Role) -> File {
        File {
            sha256: "a".repeat(64),
            bytes: 12,
            roles: [role].into(),
        }
    }

    #[test]
    fn shared_model_dependencies_retain_all_roles() -> anyhow::Result<()> {
        let mut inventory = BTreeMap::new();
        merge_dependencies(
            &mut inventory,
            [
                ("shared.bin".into(), file(Role::Mesh)),
                ("shared.bin".into(), file(Role::Data)),
            ],
        )?;
        assert_eq!(inventory.len(), 1);
        assert_eq!(
            inventory["shared.bin"].roles,
            [Role::Mesh, Role::Data].into()
        );
        Ok(())
    }

    #[test]
    fn conflicting_model_dependencies_are_rejected() {
        for mismatch in ["digest", "size"] {
            let mut inventory = BTreeMap::from([("shared.bin".into(), file(Role::Mesh))]);
            let mut conflicting = file(Role::Data);
            if mismatch == "digest" {
                conflicting.sha256 = "b".repeat(64);
            } else {
                conflicting.bytes += 1;
            }
            let error = merge_dependencies(&mut inventory, [("shared.bin".into(), conflicting)])
                .unwrap_err();
            assert!(error.to_string().contains("shared.bin"), "{error}");
            assert_eq!(inventory["shared.bin"].roles, [Role::Mesh].into());
        }
    }
}

/// Resolve the weapon resource before loading its dependencies.
pub fn weapon(files: &Files, item: u16) -> Result<battle_model::Weapon> {
    selected_weapons(files.json(battle_model::WEAPONS_PATH)?, [item])?
        .remove(&item)
        .with_context(|| format!("missing weapon resource {item}"))
}

/// Retain only selected descriptors from the encounter's decoded catalogue.
pub(super) fn selected_weapons(
    mut bank: battle_model::Weapons,
    items: impl IntoIterator<Item = u16>,
) -> Result<BTreeMap<u16, battle_model::Weapon>> {
    let mut selected = BTreeMap::new();
    for item in items {
        if selected.contains_key(&item) {
            continue;
        }
        let attachment = bank
            .records
            .remove(&item)
            .with_context(|| format!("missing weapon resource {item}"))?;
        let source = match attachment {
            battle_model::Attachment::Weapon(source) | battle_model::Attachment::Shield(source) => {
                source
            }
            battle_model::Attachment::Nonvisual => {
                anyhow::bail!("equipment {item} has no attachment model")
            }
        };
        selected.insert(item, source);
    }
    Ok(selected)
}

/// Prepared presentation ID, entry playback and controller root-axis policy.
#[derive(Debug, Clone)]
pub struct ModelSetup {
    pub resource: u32,
    pub initial: Playback,
    pub suppress_root_translation: [bool; 3],
}

/// Shared roles in the source animation catalogue. Resolve these only at preparation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MotionRole {
    Idle = 0,
    Walk = 1,
    Hurt = 3,
    Defeated = 7,
    Chant = 11,
    Cast = 12,
    Jump = 14,
    Fall = 15,
    Landing = 16,
    Breakfall = 17,
    Stop = 18,
    Run = 19,
    Item = 20,
    Entry = 24,
    WeakIdle = 26,
    Taunt = 27,
    Backstep = 28,
}

impl MotionRole {
    pub fn id(self) -> u16 {
        self as u16
    }
}

pub(super) fn common_motion(
    model: Option<&ModelDefinition>,
    role: MotionRole,
) -> Option<resonance_battle::MotionBinding> {
    motion(model, role.id()).or_else(|| {
        (role == MotionRole::Run)
            .then(|| motion(model, MotionRole::Walk.id()))
            .flatten()
    })
}

/// Resolve a visual pose only when the actor has that artwork.
pub fn motion(
    model: Option<&ModelDefinition>,
    clip: u16,
) -> Option<resonance_battle::MotionBinding> {
    let model = model?;
    model
        .motions
        .contains_key(&clip)
        .then_some(resonance_battle::MotionBinding {
            model: model.resource,
            clip,
        })
}

/// Admit clips independently; the initial pose is required when the model is constructed.
pub(super) fn motions(
    files: &Files,
    scene: &resonance_content::ScenePart,
    skeleton: &resonance_content::animation::Skeleton,
) -> Result<BTreeMap<u16, Motion>> {
    skeleton.validate()?;
    let mut motions = BTreeMap::new();
    for clip in &scene.clips {
        let decoded = (|| -> Result<Motion> {
            let motion = Motion::decode(&files.read(&clip.motion)?)?;
            motion.validate(skeleton)?;
            Ok(motion)
        })();
        if let Some(motion) = files.diagnostics().attempt(
            "battle motion",
            decoded.with_context(|| format!("clip {} ({})", clip.resource_slot, clip.motion)),
        )? {
            ensure!(
                motions.insert(clip.resource_slot, motion).is_none(),
                "duplicate battle motion slot"
            );
        }
    }
    Ok(motions)
}

/// Prepare artwork against an immutable actor; gameplay traits are already installed.
pub(super) fn body(
    files: &Files,
    rig: &battle_model::Rig,
    scene: &resonance_content::ScenePart,
    profile: &resonance_content::battle_profile::Profile,
    motions: BTreeMap<u16, Motion>,
    actor: &Actor,
    setup: ModelSetup,
) -> Result<Arc<ModelDefinition>> {
    ensure!(
        rig.skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(scene.bone_names.iter()),
        "battle body bindings differ from the shared model"
    );
    let clip = |id| motions.contains_key(&id).then_some(id);
    let hurt = if !profile.traits.suppress_hurt_motion {
        clip(MotionRole::Hurt.id())
    } else {
        None
    };
    let alternate = if !profile.traits.suppress_hurt_motion {
        clip(4).or(hurt)
    } else {
        None
    };
    let reactions = if profile.traits.body_motion_disabled {
        ReactionMotions::default()
    } else {
        ReactionMotions {
            chant: clip(MotionRole::Chant.id()).or_else(|| clip(MotionRole::Cast.id())),
            cast: clip(MotionRole::Cast.id()),
            jump: clip(MotionRole::Jump.id()),
            backstep: clip(MotionRole::Backstep.id()),
            taunt: clip(MotionRole::Taunt.id()),
            returning: clip(if actor.side == Side::Enemy {
                MotionRole::Walk.id()
            } else {
                MotionRole::Run.id()
            }),
            stopping: (actor.side == Side::Party)
                .then(|| clip(MotionRole::Stop.id()))
                .flatten(),
            hurt: [hurt, alternate],
            guard: [clip(2), clip(4)],
            stunned: clip(21),
            breakfall: clip(MotionRole::Breakfall.id()),
            airborne: clip(MotionRole::Fall.id()),
            landing: clip(MotionRole::Landing.id()),
            rising: clip(5),
            falling: clip(6),
            down: clip(8).or_else(|| clip(MotionRole::Defeated.id())),
            get_up: clip(10).or_else(|| clip(9)),
            defeated: (profile.death_motion != 0)
                .then(|| clip(u16::from(profile.death_motion)))
                .flatten()
                .or_else(|| clip(MotionRole::Defeated.id())),
        }
    };
    let blink = if let Some(blink) = &profile.blink {
        files.diagnostics().attempt(
            "battle blink",
            blink
                .validate(&profile.texture_channels)
                .map(|()| blink.clone()),
        )?
    } else {
        None
    };
    let model = ModelDefinition {
        tint: [64, 64, 64, profile.body_alpha],
        fade_on_defeat: profile.death_motion == 0 && !profile.traits.retain_defeated_body,
        secondary_motion: scene.secondary_motion.chains.clone(),
        resource: setup.resource,
        initial: setup.initial,
        suppress_root_translation: setup.suppress_root_translation,
        reactions,
        idle_motions: [
            clip(MotionRole::Idle.id()),
            if actor.side == Side::Party {
                clip(MotionRole::WeakIdle.id())
            } else {
                None
            },
        ],
        idle_expression: profile.idle_expression,
        blink,
        shadow: if actor.side == resonance_battle::Side::Enemy && profile.traits.hide_enemy_shadow {
            None
        } else {
            Some(resonance_battle::ShadowDefinition {
                scale: profile.shadow_scale,
                color: profile.shadow_color,
            })
        },
        weapons: vec![],
        skeleton: rig.skeleton.clone(),
        motions,
    };
    Ok(Arc::new(model))
}

/// Prepare one verified ordinary or stored model, sharing its presentation resource.
pub fn effect(
    files: &Files,
    part: &resonance_content::battle_model::ModelPart,
    resource: u32,
) -> Result<resonance_battle::PreparedEffectModel> {
    let primary = &part
        .layers
        .first()
        .context("missing effect model layer")?
        .scene;
    for layer in &part.layers {
        ensure!(
            part.rig
                .skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(layer.scene.bone_names.iter())
                && primary
                    .clips
                    .iter()
                    .map(|clip| (clip.resource_slot, &clip.motion))
                    .eq(layer
                        .scene
                        .clips
                        .iter()
                        .map(|clip| (clip.resource_slot, &clip.motion))),
            "effect model layer bindings differ"
        );
    }
    let mut motions = BTreeMap::new();
    for clip in &primary.clips {
        ensure!(
            motions
                .insert(
                    clip.resource_slot,
                    Motion::decode(&files.read(&clip.motion)?)?
                )
                .is_none(),
            "duplicate effect motion slot"
        );
    }
    let model = resonance_battle::PreparedEffectModel::new(Arc::new(
        resonance_battle::EffectModelDefinition {
            resource,
            skeleton: part.rig.skeleton.clone(),
            motions,
            secondary_motion: primary.secondary_motion.chains.clone(),
        },
    ))?;
    Ok(model)
}
