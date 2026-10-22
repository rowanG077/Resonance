use super::*;
use crate::battle;
use resonance_battle::conditions::ConditionSet;
use resonance_battle::{Affinity, Battle, BattleInput, Collider, PreparedBattle};
use resonance_content::{
    animation::{Bone, Skeleton, Transform, TransformChannels},
    battle_model::{Enemy, Rig},
    battle_profile,
    monster::{Monster, MonsterStats},
};

fn setup(resource: u32) -> ModelSetup {
    ModelSetup {
        resource,
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        suppress_root_translation: [false; 3],
    }
}

fn prepare_body(
    files: &Files,
    monster: &Monster,
    source: &Enemy,
    variant: usize,
    difficulty: u8,
    setup: ModelSetup,
) -> Result<(Actor, Arc<ModelDefinition>)> {
    let definition: resonance_content::battle_enemy::Definition =
        files.json(&resonance_content::battle_enemy::path(monster.id))?;
    let statistics = monster
        .statistics
        .get(variant)
        .context("missing fixture variant")?;
    let actor = battle::enemy::actor(
        monster,
        &definition.profile,
        definition.guard_recovery_bonus,
        battle::enemy::statistics(statistics, difficulty)?,
        &battle::recoil::Parameters::load(files)?,
    )?;
    let model = body(
        files,
        &source.body,
        &monster.preview.parts[0].scene,
        &definition.profile,
        motions(
            files,
            &monster.preview.parts[0].scene,
            &source.body.skeleton,
        )?,
        &actor,
        setup,
    )?;
    Ok((actor, model))
}

fn profile() -> battle_profile::Profile {
    battle_profile::Profile {
        hosted_body: None,
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 8,
        idle_ticks: 0,
        idle_variation: 0,
        initial_motion: 0,
        initial_motion_override: 0,
        body_alpha: 255,
        texture_channels: vec![],
        blink: None,
        idle_expression: [0; 4],
        active_expression: [0; 4],
        rescue_expression: [0; 4],
        weapon_styles: Default::default(),
        initial_conditions: ConditionSet::EMPTY,
        immunities: ConditionSet::of(&[
            resonance_battle::conditions::Condition::Stun,
            resonance_battle::conditions::Condition::ShortStun,
        ]),
        intrinsic_conditions: ConditionSet::EMPTY,
        weight: 2,
        species: 6,
        stun_resistance: 35,
        stagger_threshold: 100,
        stagger_ticks: 45,
        guard_reduction: 75,
        guard_pressure_limit: 999,
        traits: battle_profile::ProfileTraits {
            flying: true,
            fixed_height: true,
            clear_pending_on_hit: true,
            recover_in_air: true,
            ..Default::default()
        },
        armor: 3,
        center_offset: [10., 80., -5.],
        target_anchor: battle_profile::TargetAnchor {
            bone: 0,
            offset: [0.; 3],
        },
        head_anchor: battle_profile::HeadAnchor {
            bone: 0,
            offset: [0.; 3],
        },
        model_scale: 1.5,
        shadow_scale: 1.,
        shadow_color: [0; 4],
        effect_scale: 0.9,
        passive_effects: vec![],
        idle_effect: None,
        movement_effect: None,
        camera_category: 3,
        voices: None,
        death_motion: 0,
        overlimit_bone: 0,
        overlimit_scale: 1.,
        overlimit_gain: 0,
        initial_overlimit: 0,
        ground_offset: 50.,
        cast_ticks: 90,
        chant_once: false,
        chant_loop_start: 0.,
        cast_poses: resonance_content::battle_profile::CastPoses {
            release_loop_start: 0.,
            opening: resonance_content::battle_action::ActionPose {
                clip: 13,
                start: 0.,
                end: None,
                rate: 0.5,
                blend: 4,
                repeat: false,
            },
            release: resonance_content::battle_action::ActionPose {
                clip: 12,
                start: 0.,
                end: None,
                rate: 0.5,
                blend: 4,
                repeat: false,
            },
        },
        cast_recovery_motion: None,
    }
}

fn snapshot() -> Result<(Files, Monster, Enemy)> {
    let stats = MonsterStats {
        hp: 500,
        initial_hp: 0,
        tp: 90,
        initial_tp: 0,
        attack: 100,
        thrust: 80,
        defense: 30,
        intelligence: 40,
        accuracy: 50,
        evasion: 20,
        luck: 25,
        level: 10,
        experience: 0,
        gald: 0,
    };
    let mut monster = Monster {
        unseen_count_group: 0,
        version: resonance_content::monster::MONSTER_VERSION,
        id: 49,
        name: "enemy".into(),
        category: String::new(),
        location: String::new(),
        statistics: vec![
            stats.clone(),
            MonsterStats {
                initial_hp: 120,
                initial_tp: 20,
                ..stats
            },
        ],
        drops: [None; 2],
        drop_chances: [0; 2],
        grade: 0,
        steal: None,
        steal_chance: 0,
        attack_element: Some(resonance_battle::Element::Fire),
        affinities: [0, 1, 2, 3, 4, -1, 8, 0, 0],
        weaknesses: vec![],
        resistances: vec![],
        preview: serde_json::from_value(serde_json::json!({
            "scale":1.,"elevation":0.,"hidden_geometry":[],"behavior":null,"parts":[{
                "scene":{"resource":0,"mesh":"meshes/enemy.glb","textures":[],"materials":[],
                    "translation":[0.,0.,0.],"clips":[],"autoplay":false,"texture_animations":[],
                    "bone_names":["root","body"]},"attached_to":null,"additive":false}]
        }))?,
    };
    let mut files = Files::default();
    files.insert(
        resonance_content::battle_recoil::PATH.into(),
        serde_json::to_vec(&resonance_content::battle_recoil::Table {
            impulses: vec![[0.; 2]; 19],
            suppression_distance: 0.,
            guard_speed: 3.,
            source_sha256: "a".repeat(64),
            light_vertical_scale: 1.15,
            heavy_vertical_scale: 0.75,
            default_guard_recovery_bonuses: vec![10, 0, 5, 25, 10],
        })?
        .into(),
    );
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![],
    };
    for slot in [0, 2, 3, 4, 7, 9, 21] {
        let path = format!("motion/{slot}.motion");
        files.insert(path.clone(), motion.encode()?.into());
        monster.preview.parts[0]
            .scene
            .clips
            .push(resonance_content::SceneClip {
                resource_slot: slot,
                motion: path,
                duration_seconds: 1.,
                animation_resource: None,
                secondary_pose_nodes: vec![],
            });
    }
    let mut profile = profile();
    profile.traits = Default::default();
    files.insert(
        resonance_content::battle_enemy::path(monster.id),
        serde_json::to_vec(&resonance_content::battle_enemy::Definition {
            null_motions: Default::default(),
            source_sha256: "a".repeat(64),
            profile,
            weapon_visibility: Default::default(),
            projectiles: Vec::new(),
            actions: Default::default(),
            target_strategy: Some(resonance_battle::TargetPolicy::Nearest),
            entry_row: resonance_content::battle_enemy::EntryRow::Random,
            guard_recovery_bonus: 10,
            presentation: None,
        })?
        .into(),
    );
    let source = Enemy {
        secondary_body: None,
        body_variant: None,
        attachments: Default::default(),
        uv_animation: Default::default(),
        trails: Default::default(),
        weapon_variants: Default::default(),
        source_sha256: "a".repeat(64),
        files: Default::default(),
        body: Rig {
            skeleton: Skeleton {
                bones: vec![
                    Bone {
                        name: "root".into(),
                        parent: None,
                        bind_channels: TransformChannels(0),
                        bind: Transform::default(),
                    },
                    Bone {
                        name: "body".into(),
                        parent: Some(0),
                        bind_channels: TransformChannels(8),
                        bind: Transform {
                            translation: [0., 10., 0.],
                            ..Default::default()
                        },
                    },
                ],
            },
            attachments: Default::default(),
        },
    };
    Ok((files, monster, source))
}

#[test]
fn body_spacing_uses_scaled_body_markers_without_hurt_only_bones() -> Result<()> {
    let (_, _, mut source) = snapshot()?;
    source.body.skeleton.bones[0].name = "Mo07one_tail05".into();
    source.body.skeleton.bones[1].name = "dm3_Bone_head".into();
    let spheres = marker_spheres(&source.body.skeleton, 1.5, &[b"mo"])?;
    assert_eq!(spheres.len(), 1);
    assert_eq!(spheres[0].bone, 0);
    assert_eq!(spheres[0].radius, 105.);
    let reception = marker_spheres(&source.body.skeleton, 1.5, &[b"mo", b"dm"])?;
    assert_eq!(reception.len(), 2);
    assert_eq!(reception[0].radius, 105.);
    assert_eq!((reception[1].bone, reception[1].radius), (1, 45.));

    source.body.skeleton.bones[0].name = "mon_hair".into();
    assert!(marker_spheres(&source.body.skeleton, 1.5, &[b"mo"])?.is_empty());
    source.body.skeleton.bones[0].name = "mo16_Bone_body".into();
    assert!(marker_spheres(&source.body.skeleton, 1.5, &[b"mo"]).is_err());
    Ok(())
}

#[test]
fn body_motion_disabled_preserves_controller_pose_during_idle_settling() -> Result<()> {
    let (mut files, monster, enemy) = snapshot()?;
    let path = resonance_content::battle_enemy::path(monster.id);
    let mut source: resonance_content::battle_enemy::Definition = files.json(&path)?;
    for disabled in [false, true] {
        source.profile.traits.body_motion_disabled = disabled;
        files.insert(path.clone(), serde_json::to_vec(&source)?.into());
        let (mut actor, model) = prepare_body(&files, &monster, &enemy, 0, 0, setup(7))?;
        actor.position = [300., 0., 0.];
        let mut party = actor.clone();
        party.side = Side::Party;
        party.control = resonance_battle::Control::Manual;
        party.position[0] = -300.;
        let mut battle = PreparedBattle::new(
            vec![(party, Default::default()), (actor, Default::default())],
            Default::default(),
            1,
        )?
        .finish()?;
        battle.set_models(resonance_battle::Models::new(
            battle.actors(),
            vec![None, Some(model)],
            BTreeMap::new(),
            files.diagnostics().clone(),
        )?);
        for update in 0..2 {
            let mut frame = battle.step(BattleInput::default())?;
            if update == 0 {
                frame
                    .model_requests
                    .push(resonance_battle::ModelRequest::Play {
                        actor: resonance_battle::ActorId::from_index(1)?,
                        motion: resonance_battle::MotionBinding { model: 7, clip: 3 },
                        pose: resonance_battle::Pose::default(),
                    });
            }
            battle.advance_models(&mut frame, resonance_battle::BattleClock::Running, true)?;
            assert_eq!(frame.models[0].clip, if disabled { 3 } else { 0 });
        }
    }
    Ok(())
}

#[test]
fn optional_motion_failures_are_local_until_the_initial_clip_is_used() -> Result<()> {
    use resonance_content::diagnostics::Diagnostics;
    let (snapshot, monster, enemy) = snapshot()?;
    let (_actor, model) = prepare_body(&snapshot, &monster, &enemy, 0, 0, setup(7))?;
    let mut scene = monster.preview.parts[0].scene.clone();
    let mut clip = scene.clips[0].clone();
    clip.resource_slot = 66;
    clip.motion = "motion/broken.motion".into();
    scene.clips.push(clip);
    let mut later = scene.clips[0].clone();
    later.resource_slot = 71;
    scene.clips.push(later);
    for paranoid in [false, true] {
        let mut files = Files::new(Diagnostics::new(paranoid));
        for (path, bytes) in snapshot.iter() {
            files.insert(path.clone(), bytes.clone());
        }
        files.insert("motion/broken.motion".into(), vec![0].into());
        let result = motions(&files, &scene, &enemy.body.skeleton);
        if paranoid {
            assert!(format!("{:#}", result.unwrap_err()).contains("clip 66"));
            continue;
        }
        let admitted = result?;
        assert!(!admitted.contains_key(&66));
        assert!(admitted.contains_key(&71));
        assert!(files.diagnostics().entries()[0].message.contains("clip 66"));
        let activate = |motions| {
            let mut model = (*model).clone();
            model.motions = motions;
            model.validate_bindings()
        };
        activate(admitted)?;
        files.remove(&scene.clips[0].motion);
        assert!(activate(motions(&files, &scene, &enemy.body.skeleton)?).is_err());
    }
    Ok(())
}

#[test]
fn low_health_enemy_entry_never_selects_the_party_weak_pose() -> Result<()> {
    let mut motions = BTreeMap::from([(
        0,
        Motion {
            duration_frames: 30.,
            tracks: vec![],
        },
    )]);
    let entry = battle_profile::Entry {
        replacement_motion_rate: 0.5,
        ..Default::default()
    };
    let select = |side, motions: &BTreeMap<u16, Motion>| {
        battle::entry::initial_playback(
            &mut resonance_battle::Random::new(1),
            &entry,
            &profile(),
            side,
            motions,
            (1, 100),
            false,
        )
    };
    assert_eq!(select(Side::Enemy, &motions)?.clip, 0);
    motions.insert(MotionRole::WeakIdle.id(), motions[&0].clone());
    assert_eq!(select(Side::Enemy, &motions)?.clip, 0);
    assert_eq!(
        select(Side::Party, &motions)?.clip,
        MotionRole::WeakIdle.id()
    );
    motions.remove(&0);
    assert!(select(Side::Enemy, &motions).is_err());
    Ok(())
}

fn load(files: &Files, monster: &Monster, source: &Enemy, variant: usize) -> Result<Battle> {
    let (actor, model) = prepare_body(files, monster, source, variant, 0, setup(7))?;
    model.validate_bindings()?;
    PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1)?.finish()
}

#[test]
fn enemy_actor_preparation_is_independent_of_body_art() -> Result<()> {
    let (files, mut monster, _) = snapshot()?;
    let mut source: resonance_content::battle_enemy::Definition =
        files.json(&resonance_content::battle_enemy::path(monster.id))?;
    let recoil = battle::recoil::Parameters::load(&files)?;
    source.profile.initial_overlimit = 999;
    source.guard_recovery_bonus = 25;
    monster.preview.parts.clear();
    for (variant, expected) in [(0, (500, 90)), (1, (120, 20))] {
        let actor = battle::enemy::actor(
            &monster,
            &source.profile,
            source.guard_recovery_bonus,
            battle::enemy::statistics(&monster.statistics[variant], 0)?,
            &recoil,
        )?;
        assert_eq!((actor.hp, actor.tp), expected);
        assert_eq!(
            actor.guard.break_pressure,
            source.profile.guard_pressure_limit
        );
        assert_eq!(actor.guard.recovery_bonus, 25);
        assert_eq!(actor.overlimit.charge(), 999);
        assert_eq!(actor.equipment.base_element, monster.attack_element);
        assert_eq!(
            &actor.equipment.affinities[..5],
            &[
                Affinity::Normal,
                Affinity::Weak,
                Affinity::Resistant,
                Affinity::Absorb,
                Affinity::Immune,
            ]
        );
        assert_eq!(actor.body.scale, source.profile.model_scale);
        assert_eq!(actor.body.collider, Some(Collider::standing(35., 160.)));
        let mut active =
            PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1)?
                .finish()?;
        assert!(active.step(BattleInput::default())?.models.is_empty());
    }
    Ok(())
}

#[test]
fn enemy_art_validates_its_skeleton_and_omits_bad_blink_feedback() -> Result<()> {
    let (files, monster, source) = snapshot()?;
    for paranoid in [false, true] {
        let mut candidate = Files::new(resonance_content::diagnostics::Diagnostics::new(paranoid));
        for (path, bytes) in files.iter() {
            candidate.insert(path.clone(), bytes.clone());
        }
        let mut definition: resonance_content::battle_enemy::Definition =
            candidate.json(&resonance_content::battle_enemy::path(monster.id))?;
        definition.profile.blink = Some(battle_profile::Blink {
            channel: 0,
            frames: [1, 2],
            excluded_expressions: vec![],
        });
        candidate.insert(
            resonance_content::battle_enemy::path(monster.id),
            serde_json::to_vec(&definition)?.into(),
        );
        let prepared = prepare_body(&candidate, &monster, &source, 0, 0, setup(7));
        if paranoid {
            assert!(prepared.is_err());
        } else {
            assert!(prepared?.1.blink.is_none());
            load(&candidate, &monster, &source, 0)?.step(BattleInput::default())?;
        }
        assert!(!candidate.diagnostics().entries().is_empty());
    }
    for change in [
        |s: &mut Enemy| s.body.skeleton.bones[1].name = "different".into(),
        |s: &mut Enemy| s.body.skeleton.bones[1].parent = Some(1),
    ] {
        let mut changed = source.clone();
        change(&mut changed);
        assert!(load(&files, &monster, &changed, 0).is_err());
    }
    let mut duplicate = monster.clone();
    duplicate.preview.parts[0]
        .scene
        .clips
        .push(monster.preview.parts[0].scene.clips[0].clone());
    assert!(load(&files, &duplicate, &source, 0).is_err());
    assert!(load(&files, &monster, &source, 2).is_err());
    Ok(())
}

#[test]
fn effect_pose_sharing_uses_indices_without_ignoring_transform_differences() {
    let primary = Skeleton {
        bones: vec![
            Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            },
            Bone {
                name: "body".into(),
                parent: Some(0),
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            },
        ],
    };
    let mut outline = primary.clone();
    outline.bones[1].name = "outline".into();
    assert!(shares_indexed_pose(&primary, &outline));
    outline.bones[1].bind.translation[0] = 0.000001;
    outline.bones[1].bind.scale[1] += f32::EPSILON;
    assert!(shares_indexed_pose(&primary, &outline));
    outline.bones[1].parent = None;
    assert!(!shares_indexed_pose(&primary, &outline));
    outline.bones[1].parent = Some(0);
    outline.bones[1].bind.translation[0] = 1.;
    assert!(!shares_indexed_pose(&primary, &outline));
}

#[test]
#[ignore = "requires cooked enemy 134 effect model"]
fn differently_named_effect_outline_uses_the_verified_indexed_pose() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let bank: resonance_content::battle_effect::SourceBank = serde_json::from_slice(
        &std::fs::read(root.join(battle_model::enemy_effects_path(134)))?,
    )?;
    let art = bank.art.context("missing effect art")?;
    let mut files = Files::default();
    for path in art.files.keys() {
        files.insert(path.clone(), std::fs::read(root.join(path))?.into());
    }
    for part in art.models.values() {
        effect(&files, part, 0, false)?;
        let mut invalid = part.clone();
        invalid.rig.skeleton.bones[0].bind.translation[0] += 1.;
        assert!(effect(&files, &invalid, 0, false).is_err());
    }
    Ok(())
}
