use super::*;
use crate::battle;
use resonance_battle::conditions::ConditionSet;
use resonance_battle::{
    Affinity, Battle, BattleInput, Collider, Difficulty, PreparedBattle, ResourceId,
};
use resonance_content::{
    animation::{Bone, Skeleton, Transform},
    battle_model::{Enemy, Rig},
    battle_profile,
    monster::{Monster, MonsterStats},
};

fn setup(resource: u32) -> ModelSetup {
    ModelSetup {
        resource: ResourceId(resource),
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
    setup: ModelSetup,
) -> Result<(Actor, Arc<ModelDefinition>)> {
    let definition: resonance_content::battle_enemy::document::Rules =
        files.json(&"battle/enemies/test-enemy/rules.json".to_owned())?;
    let statistics = monster
        .statistics
        .get(variant)
        .context("missing fixture variant")?;
    let actor = battle::enemy::actor(
        monster,
        &definition.profile,
        definition.guard_recovery_bonus,
        battle::enemy::statistics(statistics, Difficulty::Normal),
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
        expressions: vec![],
        weapon_styles: Default::default(),
        initial_conditions: ConditionSet::EMPTY,
        immunities: ConditionSet::of(&[
            resonance_battle::conditions::Condition::Stun,
            resonance_battle::conditions::Condition::ShortStun,
        ]),
        intrinsic_conditions: ConditionSet::EMPTY,
        recoil_weight: battle_profile::RecoilWeight::Heavy,
        species: resonance_content::battle_profile::Species::Human,
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
        target_anchor: battle_profile::TargetAnchor::Reference(
            battle_profile::TargetOrigin::Center,
        ),
        head_anchor: battle_profile::HeadAnchor {
            bone: "root".into(),
            offset: [0.; 3],
        },
        model_scale: 1.5,
        shadow: None,
        effect_scale: 0.9,
        passive_effects: vec![],
        idle_effect: None,
        movement_effect: None,
        size: battle_profile::Size::Huge,
        voices: None,
        death_motion: 0,
        overlimit_bone: "root".into(),
        overlimit_scale: 1.,
        overlimit_gain: 0,
        initial_overlimit: 0,
        ground_offset: 50.,
        casting: battle_profile::Casting {
            base_ticks: 90,
            release_start: 0.,
            ..Default::default()
        },
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
                "resource":0,"mesh":"meshes/enemy.glb","textures":[],"materials":[],
                    "translation":[0.,0.,0.],"clips":[],"autoplay":false,"texture_animations":[],
                    "attached_to":null,"additive":false}]
        }))?,
    };
    let mut files = Files::default();
    let mesh = resonance_asset_writer::gltf::pack_glb(
        &serde_json::json!({
            "asset":{"version":"2.0"},
            "nodes":[
                {"name":"root","extras":{"resonance_bone":0},"children":[1]},
                {"name":"body","extras":{"resonance_bone":1},"translation":[0,10,0]}
            ]
        }),
        &[],
    )?;
    monster.preview.parts[0].scene.prepare_bones(&mesh)?;
    files.insert("meshes/enemy.glb".into(), mesh.into());
    files.insert(resonance_content::battle_model::document::PARTY.into(),br#"{"1":"lloyd","2":"colette","3":"genis","4":"raine","5":"sheena","6":"zelos","7":"presea","8":"regal","9":"kratos"}"#.to_vec().into());
    files.insert(
        resonance_content::battle_recoil::PATH.into(),
        serde_json::to_vec(&resonance_content::battle_rules::Recoil {
            impulses: (0..19)
                .map(|id| {
                    (
                        id,
                        resonance_content::battle_rules::RecoilImpulse {
                            horizontal: 0.,
                            vertical: 0.,
                        },
                    )
                })
                .collect(),
            suppression_distance: 0.,
            guard_speed: 3.,
            light_vertical_scale: 1.15,
            heavy_vertical_scale: 0.75,
            default_guard_recovery_bonuses: [
                ("lloyd", 0),
                ("colette", 5),
                ("genis", 25),
                ("raine", 10),
            ]
            .into_iter()
            .map(|(name, bonus)| (name.into(), bonus))
            .collect(),
        })?
        .into(),
    );
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![],
    };
    for slot in [0, 2, 3, 4, 7, 9, 21] {
        let path = format!("motion/{slot}.glb");
        files.insert(
            path.clone(),
            motion
                .encode(&resonance_content::animation::Skeleton {
                    bones: ["root", "body"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, name)| resonance_content::animation::Bone {
                            name: name.into(),
                            parent: (index == 1).then_some(0),
                            parent_transform: None,
                            shadow_bounds: true,
                            bind: Default::default(),
                        })
                        .collect(),
                })?
                .into(),
        );
        monster.preview.parts[0]
            .scene
            .clips
            .push(resonance_content::SceneClip {
                joints: Default::default(),
                resource_slot: slot,
                motion: path,
                duration_seconds: 1.,
                animation_resource: None,
                pose_chains: vec![],
            });
    }
    let mut profile = profile();
    profile.traits = Default::default();
    files.insert(
        "battle/enemies/test-enemy/rules.json".to_owned(),
        serde_json::to_vec(&resonance_content::battle_enemy::document::Rules {
            null_motions: Default::default(),
            profile,
            weapon_visibility: Default::default(),
            projectiles: Default::default(),
            contacts: Default::default(),
            attacks: Default::default(),
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
        attached_models: Vec::new(),
        attachments: Default::default(),
        uv_animation: Default::default(),
        trails: Default::default(),
        weapon_variants: Default::default(),

        body: Rig {
            skeleton: Skeleton {
                bones: vec![
                    Bone {
                        name: "root".into(),
                        parent: None,
                        parent_transform: None,
                        shadow_bounds: true,
                        bind: Transform::default(),
                    },
                    Bone {
                        name: "body".into(),
                        parent: Some(0),
                        parent_transform: None,
                        shadow_bounds: true,
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
fn body_motion_disabled_preserves_controller_pose_when_returning_to_idle() -> Result<()> {
    let (mut files, monster, enemy) = snapshot()?;
    let path = "battle/enemies/test-enemy/rules.json".to_owned();
    let mut source: resonance_content::battle_enemy::document::Rules = files.json(&path)?;
    for disabled in [false, true] {
        source.profile.traits.body_motion_disabled = disabled;
        files.insert(path.clone(), serde_json::to_vec(&source)?.into());
        let (mut actor, model) = prepare_body(&files, &monster, &enemy, 0, setup(7))?;
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
        let actor = resonance_battle::ActorId::from_index(1)?;
        for (request, expected_clip) in [
            (
                resonance_battle::ModelRequest::Play {
                    actor,
                    motion: resonance_battle::MotionBinding {
                        model: ResourceId(7),
                        clip: 3,
                    },
                    pose: resonance_battle::Pose::default(),
                },
                3,
            ),
            (
                resonance_battle::ModelRequest::Common {
                    actor,
                    pose: resonance_battle::CommonPose::Idle,
                },
                if disabled { 3 } else { 0 },
            ),
        ] {
            let mut frame = battle.step(BattleInput::default())?;
            frame.model_requests.push(request);
            battle.advance_models(&mut frame, resonance_battle::BattleClock::Running, true)?;
            assert_eq!(frame.models[0].clip, expected_clip);
        }
    }
    Ok(())
}

#[test]
fn optional_motion_failures_are_local_until_the_initial_clip_is_used() -> Result<()> {
    use resonance_content::diagnostics::Diagnostics;
    let (snapshot, monster, enemy) = snapshot()?;
    let (_actor, model) = prepare_body(&snapshot, &monster, &enemy, 0, setup(7))?;
    let mut scene = monster.preview.parts[0].scene.clone();
    let mut clip = scene.clips[0].clone();
    clip.resource_slot = 66;
    clip.motion = "motion/broken.glb".into();
    scene.clips.push(clip);
    let mut later = scene.clips[0].clone();
    later.resource_slot = 71;
    scene.clips.push(later);
    for paranoid in [false, true] {
        let mut files = Files::new(Diagnostics::new(paranoid));
        for (path, bytes) in snapshot.iter() {
            files.insert(path.clone(), bytes.clone());
        }
        files.insert("motion/broken.glb".into(), vec![0].into());
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
    let (actor, model) = prepare_body(files, monster, source, variant, setup(7))?;
    model.validate_bindings()?;
    PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1)?.finish()
}

#[test]
fn enemy_actor_preparation_is_independent_of_body_art() -> Result<()> {
    let (files, mut monster, _) = snapshot()?;
    let mut source: resonance_content::battle_enemy::document::Rules =
        files.json(&"battle/enemies/test-enemy/rules.json".to_owned())?;
    let recoil = battle::recoil::Parameters::load(&files)?;
    source.profile.initial_overlimit = 999;
    source.guard_recovery_bonus = 25;
    monster.preview.parts.clear();
    for (variant, expected) in [(0, (500, 90)), (1, (120, 20))] {
        let actor = battle::enemy::actor(
            &monster,
            &source.profile,
            source.guard_recovery_bonus,
            battle::enemy::statistics(&monster.statistics[variant], Difficulty::Normal),
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
        let mut definition: resonance_content::battle_enemy::document::Rules =
            candidate.json(&"battle/enemies/test-enemy/rules.json".to_owned())?;
        definition.profile.expressions =
            serde_json::from_str(r#"[{"texture":0,"frames":2,"blink":[1,2]}]"#)?;
        candidate.insert(
            "battle/enemies/test-enemy/rules.json".to_owned(),
            serde_json::to_vec(&definition)?.into(),
        );
        let prepared = prepare_body(&candidate, &monster, &source, 0, setup(7));
        if paranoid {
            assert!(prepared.is_err());
        } else {
            assert!(prepared?.1.blinks.is_empty());
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
                parent_transform: None,
                shadow_bounds: true,
                bind: Transform::default(),
            },
            Bone {
                name: "body".into(),
                parent: Some(0),
                parent_transform: None,
                shadow_bounds: true,
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
#[ignore = "requires current RESONANCE_TEST_ASSETS with enemy 134 effect model"]
fn differently_named_effect_outline_uses_the_verified_indexed_pose() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let mut snapshot = resonance_content::library::Library::open(&root, &[])?.snapshot();
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    let captured = resonance_content::battle_enemy::document::capture(
        &mut snapshot,
        134,
        &diagnostics,
        &|| false,
    )?;
    let art = captured
        .effects
        .context("missing effect bank")?
        .1
        .art
        .context("missing effect art")?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    for part in art.models.values() {
        effect(&files, part, ResourceId(0), false)?;
        let mut invalid = part.clone();
        invalid.rig.skeleton.bones[0].bind.translation[0] += 1.;
        assert!(effect(&files, &invalid, ResourceId(0), false).is_err());
    }
    Ok(())
}
