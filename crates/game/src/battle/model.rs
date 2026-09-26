//! Verified battle body preparation. Rendering, placement and entry clocks are
//! supplied by encounter preparation; the core owns subsequent pose sampling.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Actor, Affinity, Anchor, AttackElements, Body, CombatStats, Control, HurtPoint,
    KnockdownBinding, ModelDefinition, ParticleDefinition, Playback, Side, SoundBinding,
    StunBinding,
};
use resonance_content::{animation::Motion, battle_model, monster::Monster, prepared::Files};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Debug, Clone, Copy)]
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
    let mut inventory: BTreeMap<String, resonance_content::field_preload::File> = BTreeMap::new();
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
    }
    files.with_dependencies(root, inventory, cache, cancelled)
}

/// Resolve the original weapon resource domain before loading its dependencies.
/// Native callers 159BC/16060 select equipment and battle-only models separately.
pub fn weapon(files: &Files, item: u16) -> Result<battle_model::Weapon> {
    let index = match item {
        135..=273 => item - 135,
        356..=366 => item - 217,
        528.. => item - 379,
        _ => anyhow::bail!("invalid weapon resource {item}"),
    };
    let mut bank: battle_model::Weapons = files.json(battle_model::WEAPONS_PATH)?;
    bank.records
        .get_mut(usize::from(index))
        .and_then(Option::take)
        .with_context(|| format!("missing weapon resource {item}"))
}

/// Bind an ordinary rigid weapon instance to its owner's attachment bone.
/// The caller selects the instance and controller policy; owner-linked or
/// independently animated weapons must use their own sampled pose.
/// Returned groups are relative to this instance, as in the original 153BC.
pub fn rigid_weapon(
    model: &mut ModelDefinition,
    weapon: &battle_model::ModelPart,
    attachment: u16,
) -> Result<Vec<Vec<u16>>> {
    ensure!(
        usize::from(attachment) < model.skeleton.bones.len(),
        "invalid weapon attachment bone"
    );
    ensure!(
        !weapon.layers.is_empty()
            && weapon
                .layers
                .iter()
                .all(|layer| layer.scene.clips.is_empty()),
        "rigid weapon requires unanimated layers"
    );
    let rig = &weapon.rig;
    ensure!(
        rig.transform_kinds.len() == rig.skeleton.bones.len()
            && rig.transform_kinds.iter().all(|&kind| kind == 1),
        "unsupported weapon bone transform"
    );
    rig.skeleton.validate()?;
    let pose = rig.skeleton.bind_pose()?;
    let count = rig
        .attack_groups
        .keys()
        .next_back()
        .map_or(0, |&id| usize::from(id) + 1);
    ensure!(count <= 12, "invalid weapon contact group");
    let mut groups = vec![Vec::new(); count];
    let mut anchors = Vec::new();
    for (&group, bones) in &rig.attack_groups {
        ensure!(bones.len() <= 7, "too many weapon contact bones");
        for &bone in bones {
            let index = u16::try_from(model.anchors.len() + anchors.len())?;
            anchors.push(Anchor {
                bone: attachment,
                offset: pose.point(bone, [0.; 3])?,
            });
            groups[usize::from(group)].push(index);
        }
    }
    ensure!(
        model.anchors.len() + anchors.len() <= 256,
        "too many battle anchors"
    );
    model.anchors.extend(anchors);
    Ok(groups)
}

/// Common stun particle and sound already prepared by the presentation loader.
pub struct StunResources {
    pub particle: Arc<ParticleDefinition>,
    pub sound: SoundBinding,
}

/// Prepared presentation ID, entry playback and controller root-axis policy.
pub struct ModelSetup {
    pub resource: u32,
    pub initial: Playback,
    pub suppress_root_translation: [bool; 3],
    pub stun: Option<StunResources>,
}

pub fn party(
    files: &Files,
    character: u8,
    actor: Actor,
    setup: ModelSetup,
) -> Result<(Actor, Arc<ModelDefinition>)> {
    ensure!(
        actor.side == Side::Party,
        "party model requires a party actor"
    );
    let source: battle_model::Party = files.json(&battle_model::party_path(character))?;
    let profile = super::profile::party_template(files, character)?;
    body(
        files,
        source.body,
        source.parts.first().context("missing party body scene")?,
        &profile,
        actor,
        setup,
    )
}

/// Prepare the ordinary enemy body and base statistics. Difficulty, active
/// conditions, AI, attachments and encounter placement are separate consumers.
/// Initial playback and root-axis policy belong to the entry/controller binding.
pub fn enemy(
    files: &Files,
    monster: &Monster,
    variant: usize,
    setup: ModelSetup,
) -> Result<(Actor, Arc<ModelDefinition>)> {
    monster.validate(528)?;
    let source: battle_model::Enemy = files.json(&battle_model::enemy_path(monster.id))?;
    let stats = monster
        .statistics
        .get(variant)
        .context("missing enemy variant")?;
    let actor = Actor {
        side: Side::Enemy,
        control: Control::Enemy,
        activity: Default::default(),
        availability: Default::default(),
        overlimit: 0,
        overlimit_active: false,
        guard: resonance_battle::Guard {
            // 44388 keeps the resource preference and clears the actor-kind
            // nibble; 1A9AC only consults kind zero when the byte is zero.
            recovery_bonus: super::recoil::Parameters::load(files)?
                .guard_recovery_bonus(source.guard_preference, 0)?,
            ..Default::default()
        },
        hp: if stats.initial_hp == 0 {
            stats.hp
        } else {
            stats.initial_hp
        }
        .try_into()?,
        max_hp: stats.hp.try_into()?,
        tp: if stats.initial_tp == 0 {
            stats.tp
        } else {
            stats.initial_tp
        },
        max_tp: stats.tp,
        hud: Default::default(),
        luck: stats.luck,
        stats: CombatStats {
            slash: stats.attack.try_into()?,
            thrust: stats.thrust,
            defense: stats.defense.try_into()?,
            intelligence: stats.intelligence,
            accuracy: stats.accuracy,
            evasion: stats.evasion,
            level: stats.level,
        },
        elements: AttackElements {
            base: monster.attack_element,
            ..Default::default()
        },
        // 61578's signed-byte switch leaves unrecognized codes unchanged.
        affinities: monster.affinities.map(|code| match code {
            1 => Affinity::Weak,
            2 => Affinity::Resistant,
            3 => Affinity::Absorb,
            4 => Affinity::Immune,
            _ => Affinity::Normal,
        }),
        attack_power: 100,
        physical_arte_boost: false,
        recovery: Default::default(),
        petrified: false,
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        framing: Default::default(),
        body: Body::default(),
        movement: Default::default(),
        reaction: Default::default(),
        hit_stop: 0,
    };
    body(
        files,
        source.body,
        &monster.preview.parts[0].scene,
        &source.profile,
        actor,
        setup,
    )
}

fn body(
    files: &Files,
    rig: battle_model::Rig,
    scene: &resonance_content::ScenePart,
    profile: &resonance_content::battle_profile::Profile,
    mut actor: Actor,
    setup: ModelSetup,
) -> Result<(Actor, Arc<ModelDefinition>)> {
    ensure!(
        rig.transform_kinds.len() == rig.skeleton.bones.len()
            && rig.transform_kinds.iter().all(|&kind| kind == 1),
        "unsupported battle bone transform"
    );
    ensure!(
        rig.skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(scene.bone_names.iter()),
        "battle body bindings differ from the shared model"
    );
    let mut motions = BTreeMap::new();
    for clip in &scene.clips {
        let motion = Motion::decode(&files.read(&clip.motion)?)?;
        ensure!(
            motions.insert(clip.resource_slot, motion).is_none(),
            "duplicate battle motion slot"
        );
    }
    actor.body.points = rig
        .volumes
        .iter()
        .filter(|volume| volume.hurt)
        .map(|volume| HurtPoint {
            center: [0.; 3],
            radius: volume.radius,
        })
        .collect();
    actor.body.approach_points = rig
        .volumes
        .iter()
        .filter(|volume| volume.body)
        .map(|volume| HurtPoint {
            center: [0.; 3],
            radius: volume.radius,
        })
        .collect();
    super::profile::apply(files, profile, &mut actor)?;
    let clip = |id| motions.contains_key(&id).then_some(id);
    let hurt = if profile.body_flags & 0x20 == 0 {
        clip(3)
    } else {
        None
    };
    let alternate = if profile.body_flags & 0x20 == 0 {
        clip(4).or(hurt)
    } else {
        None
    };
    let knockdown = if actor.reaction.profile.can_knock_down && profile.flags & 0x800 == 0 {
        clip(7).map(|down_motion| KnockdownBinding {
            down_motion,
            recovery_motion: clip(9),
        })
    } else {
        None
    };
    let stun = setup
        .stun
        .map(|resources| -> Result<_> {
            Ok(StunBinding {
                particle: resources.particle,
                sound: resources.sound,
                head: u16::from(profile.head_bone),
                offset: [
                    profile.stun_offset[0].finite()?,
                    profile.stun_offset[1].finite()?,
                    profile.stun_offset[2].finite()?,
                ],
                loop_motion: 21,
                down_motion: 7,
                recovery_motion: 9,
            })
        })
        .transpose()?;
    let model = ModelDefinition {
        secondary_motion: scene.secondary_motion.prepare(&scene.bone_names)?,
        resource: setup.resource,
        initial: setup.initial,
        suppress_root_translation: setup.suppress_root_translation,
        hurt_motions: [hurt, alternate],
        idle_motions: [
            clip(0),
            if actor.side == Side::Party {
                clip(26)
            } else {
                None
            },
        ],
        guard_motions: [clip(2), clip(4)],
        stun,
        knockdown,
        anchors: (0..rig.skeleton.bones.len())
            .map(|bone| {
                Ok(Anchor {
                    bone: u16::try_from(bone)?,
                    offset: [0.; 3],
                })
            })
            .collect::<Result<_>>()?,
        hurt_bones: rig
            .volumes
            .iter()
            .filter(|volume| volume.hurt)
            .map(|volume| volume.bone)
            .collect(),
        approach_bones: rig
            .volumes
            .iter()
            .filter(|volume| volume.body)
            .map(|volume| volume.bone)
            .collect(),
        target_bones: rig.target_bones,
        target_marker: if profile.target_bone == 0 {
            None
        } else {
            Some(Anchor {
                bone: u16::from(profile.target_bone),
                offset: [
                    profile.target_offset[0].finite()?,
                    profile.target_offset[1].finite()?,
                    profile.target_offset[2].finite()?,
                ],
            })
        },
        shadow: if actor.side == resonance_battle::Side::Enemy && profile.flags & 8 != 0 {
            None
        } else {
            Some(resonance_battle::ShadowDefinition {
                scale: profile.shadow_scale.finite()?,
                color: profile.shadow_color,
            })
        },
        weapons: vec![],
        skeleton: rig.skeleton,
        motions,
    };
    // PreparedBattle validates the complete actor/model/action set together.
    Ok((actor, Arc::new(model)))
}

/// Prepare each stored-scene model instance from the same verified files used
/// by the presentation loader. Body and outline consume the same sampled pose.
pub fn scene(
    files: &Files,
    scene: &resonance_content::battle_scene::Scene,
    resources: &BTreeMap<u8, u32>,
) -> Result<BTreeMap<u8, resonance_battle::PreparedEffectModel>> {
    ensure!(
        scene.models.keys().eq(resources.keys()),
        "scene model resource slots differ"
    );
    scene
        .models
        .iter()
        .map(|(&slot, part)| {
            let model = effect(files, part, resources[&slot])?;
            Ok((slot, model))
        })
        .collect()
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
    ensure!(
        part.rig.transform_kinds.len() == part.rig.skeleton.bones.len()
            && part.rig.transform_kinds.iter().all(|&kind| kind == 1),
        "unsupported effect bone transform"
    );
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
            secondary_motion: primary.secondary_motion.prepare(&primary.bone_names)?,
        },
    ))?;
    Ok(model)
}
