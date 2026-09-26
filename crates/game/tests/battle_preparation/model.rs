use super::*;
use anyhow::Context;
use battle::model::{ModelSetup, ModelSource};
use resonance_battle::{Affinity, Playback, PreparedBattle};
use resonance_content::{
    animation::{Bone, Motion, Skeleton, Transform, TransformChannels},
    battle_model::{self, Enemy, Rig, Volume},
    monster::{Monster, MonsterStats},
};

fn playback() -> Playback {
    Playback {
        clip: 0,
        frame: 0.,
        rate: 0.5,
        repeat: true,
    }
}

fn setup(resource: u32) -> ModelSetup {
    ModelSetup {
        resource,
        initial: playback(),
        suppress_root_translation: [false; 3],
        stun: None,
    }
}

#[test]
fn weapon_selection_keeps_equipment_domains_aliases_and_missing_slots() -> Result<()> {
    let mut files = files("");
    let mut bank = battle_model::Weapons {
        source_sha256: "a".repeat(64),
        table_sha256: "b".repeat(64),
        owner_motion_sha256: "c".repeat(64),
        records: vec![None; 156],
    };
    for index in [0, 138, 139, 149, 150] {
        bank.records[index] = Some(battle_model::Weapon {
            trails: Default::default(),
            source_sha256: index.to_string(),
            parts: Default::default(),
            files: Default::default(),
        });
    }
    files.bytes.insert(
        battle_model::WEAPONS_PATH.into(),
        serde_json::to_vec(&bank)?.into(),
    );
    for (id, index) in [
        (135, 0),
        (273, 138),
        (356, 139),
        (366, 149),
        (528, 149),
        (529, 150),
    ] {
        assert_eq!(
            battle::model::weapon(&files, id)?.source_sha256,
            index.to_string()
        );
    }
    for id in [0, 134, 136, 274, 355, 367, 527, 530, u16::MAX] {
        assert!(battle::model::weapon(&files, id).is_err(), "weapon {id}");
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked weapon publications; no devices"]
fn cold_weapon_selection_verifies_original_sword_layers_and_contact_rig() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let selected = battle::model::load_files(
        &root,
        field,
        &[ModelSource::Weapon(135)],
        &mut cache,
        || false,
    )?;
    let weapon = battle::model::weapon(&selected, 135)?;
    assert_eq!(weapon.parts.keys().copied().collect::<Vec<_>>(), [0, 1]);
    for path in weapon.files.keys() {
        selected.read(path)?;
    }
    for part in weapon.parts.values() {
        assert!(part.layers[0].scene.clips.is_empty());
        let pose = part.rig.skeleton.bind_pose()?;
        for &bone in &part.rig.attack_groups[&0] {
            assert!(
                pose.point(bone, [0.; 3])?
                    .iter()
                    .all(|value| value.is_finite())
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked party and weapon publications; no devices"]
fn cold_lloyd_rigid_weapon_and_body_poses_match_dolphin() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct WeaponPose {
        bone: u16,
        matrices: Vec<[u32; 12]>,
    }
    #[derive(serde::Deserialize)]
    struct Sample {
        tick: u32,
        clock: u16,
        clip: u16,
        frame: f32,
        position: [f32; 3],
        heading: f32,
        body: Vec<[u32; 12]>,
        weapons: Vec<WeaponPose>,
        contact_positions: Vec<[u32; 3]>,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        body_sha256: String,
        motion_sha256: String,
        weapon_sha256: String,
        samples: Vec<Sample>,
    }
    let fixture: Fixture =
        serde_json::from_str(include_str!("../fixtures/lloyd-weapon-poses.json"))?;
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        field,
        &[ModelSource::Party(1), ModelSource::Weapon(135)],
        &mut cache,
        || false,
    )?;
    let body: battle_model::Party = files.json(&battle_model::party_path(1))?;
    let weapon = battle::model::weapon(&files, 135)?;
    assert_eq!(body.body_sha256, fixture.body_sha256);
    assert_eq!(body.motion_sha256, fixture.motion_sha256);
    assert_eq!(weapon.source_sha256, fixture.weapon_sha256);
    // Hair and coat matrices also exist in this capture. They require the
    // secondary solver's retained history and are not accepted by this check.
    let secondary: std::collections::BTreeSet<_> = body.parts[0]
        .secondary_motion
        .chains
        .iter()
        .flat_map(|chain| chain.joints.iter().map(|joint| usize::from(joint.node)))
        .collect();
    let mut max_body = 0f32;
    let mut max_weapon = 0f32;
    for sample in fixture.samples {
        let mut session = actor();
        session.position = sample.position;
        session.heading = sample.heading;
        let mut setup = setup(1);
        setup.initial = Playback {
            clip: sample.clip,
            frame: sample.frame,
            rate: 0.,
            repeat: false,
        };
        setup.suppress_root_translation = [true; 3];
        let (session, mut model) = battle::model::party(&files, 1, session, setup)?;
        let mut groups = Vec::new();
        for (index, observed) in sample.weapons.iter().enumerate() {
            let slot = u8::try_from(index)?;
            assert_eq!(body.body.attachments[&slot], observed.bone);
            groups.extend(battle::weapon::attach(
                &files,
                Arc::make_mut(&mut model),
                &weapon.parts[&slot],
                slot,
                observed.bone,
                10 + u32::from(slot),
                resonance_battle::WeaponPlayback::Rigid,
            )?);
        }
        let contact = battle::melee::load(
            &files,
            &MeleeResource {
                impact: None,
                source: resonance_content::battle_action::NORMAL_PATH.into(),
                selection: battle::MeleeSelection::Normal {
                    character: 0,
                    selection: 4,
                },
                row: u8::from(sample.clock >= 17),
                anchor_groups: groups.clone(),
            },
        )?;
        let mut active = Battle::new(Arc::new(PreparedBattle::new(
            vec![session],
            vec![],
            1,
            vec![Some(model)],
            vec![],
        )?));
        // Rate zero preserves this explicitly observed frame; no input retiming.
        let frame = active.step(BattleInput::default())?;
        let shown = &frame.models[0];
        assert_eq!(frame.weapons.len(), 2);
        assert!(frame.weapons.iter().all(|weapon| weapon.clip.is_none()));
        assert_eq!(contact.anchors.len(), sample.contact_positions.len());
        for (&anchor, expected) in contact.anchors.iter().zip(&sample.contact_positions) {
            for (actual, expected) in frame.actors[0].body.anchors[usize::from(anchor)]
                .iter()
                .zip(expected)
            {
                assert!(
                    (actual - f32::from_bits(*expected)).abs() < 0.001,
                    "tick {} submitted anchor {anchor}",
                    sample.tick
                );
            }
        }
        assert_eq!(sample.body.len(), shown.bones.len());
        for (bone, expected) in sample.body.iter().enumerate() {
            if secondary.contains(&bone) {
                continue;
            }
            let matrix = resonance_content::animation::multiply(shown.world, shown.bones[bone]);
            let mut delta = 0f32;
            for row in 0..3 {
                for column in 0..4 {
                    delta = delta.max(
                        (matrix[column][row] - f32::from_bits(expected[row * 4 + column])).abs(),
                    );
                }
            }
            max_body = max_body.max(delta);
            assert!(delta < 0.001, "tick {} bone {bone}: {delta}", sample.tick);
        }
        assert_eq!(groups.len(), sample.weapons.len());
        for (group, observed) in groups.iter().zip(&sample.weapons) {
            assert_eq!(group.len(), observed.matrices.len());
            for (&anchor, matrix) in group.iter().zip(&observed.matrices) {
                let actual = frame.actors[0].body.anchors[usize::from(anchor)];
                for axis in 0..3 {
                    let delta = (actual[axis] - f32::from_bits(matrix[axis * 4 + 3])).abs();
                    max_weapon = max_weapon.max(delta);
                    assert!(
                        delta < 0.001,
                        "tick {} anchor {anchor} axis {axis}: {delta}",
                        sample.tick
                    );
                }
            }
        }
    }
    eprintln!("maximum absolute error: ordinary body {max_body}, weapon contacts {max_weapon}");
    Ok(())
}

fn snapshot() -> Result<(Files, Monster)> {
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
    let mut files = files("");
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![],
    };
    for slot in [0, 2, 3, 4, 7, 9, 21] {
        let path = format!("motion/{slot}.motion");
        files.bytes.insert(path.clone(), motion.encode()?.into());
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
    let mut profile = super::profile::profile();
    profile.flags = 0;
    profile.body_flags = 0;
    profile.head_bone = 1;
    let source = Enemy {
        name: "enemy".into(),
        hidden_name_units: 2,
        actions: Default::default(),
        target_strategy: 0,
        guard_preference: 0,
        attachments: Default::default(),
        trails: Default::default(),
        source_sha256: "a".repeat(64),
        files: Default::default(),
        profile,
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
            transform_kinds: vec![1, 1],
            target_bones: vec![],
            volumes: vec![Volume {
                bone: 1,
                radius: 80.,
                hurt: true,
                body: true,
            }],
            attack_groups: Default::default(),
            attachments: Default::default(),
        },
    };
    files.bytes.insert(
        battle_model::enemy_path(49),
        serde_json::to_vec(&source)?.into(),
    );
    Ok((files, monster))
}

#[test]
fn rigid_weapon_groups_preserve_duplicates_and_failed_binding_leaves_model_intact() -> Result<()> {
    let (files, monster) = snapshot()?;
    let source: Enemy = files.json(&battle_model::enemy_path(monster.id))?;
    let (_, model) = battle::model::enemy(&files, &monster, 0, setup(7))?;
    let mut model = (*model).clone();
    let mut scene = monster.preview.parts[0].clone();
    scene.scene.clips.clear();
    let mut weapon = battle_model::ModelPart {
        rig: source.body,
        layers: vec![scene],
    };
    weapon.rig.attack_groups = [(0, vec![0, 1]), (2, vec![1])].into();
    let groups = battle::model::rigid_weapon(&mut model, &weapon, 1)?;
    assert_eq!(groups, [vec![2, 3], vec![], vec![4]]);
    assert_eq!(model.anchors[2].bone, 1);
    assert_eq!(model.anchors[2].offset, [0.; 3]);
    assert_eq!(model.anchors[3].offset, [0., 10., 0.]);
    assert_eq!(model.anchors[4].offset, model.anchors[3].offset);
    let before: Vec<_> = model.anchors.iter().map(|a| (a.bone, a.offset)).collect();
    assert!(battle::model::rigid_weapon(&mut model, &weapon, 255).is_err());
    for fault in 0..7 {
        let mut invalid = weapon.clone();
        match fault {
            0 => invalid.rig.attack_groups.insert(2, vec![255]),
            1 => invalid.rig.attack_groups.insert(12, vec![0]),
            2 => invalid.rig.attack_groups.insert(2, vec![0; 8]),
            3 => {
                invalid.rig.transform_kinds[0] = 2;
                None
            }
            4 => {
                invalid.layers[0].scene.clips = monster.preview.parts[0].scene.clips.clone();
                None
            }
            5 => {
                invalid.rig.skeleton.bones[1].bind.translation[0] = f32::NAN;
                None
            }
            _ => {
                invalid.layers.clear();
                None
            }
        };
        assert!(
            battle::model::rigid_weapon(&mut model, &invalid, 1).is_err(),
            "fault {fault}"
        );
        assert_eq!(
            model
                .anchors
                .iter()
                .map(|a| (a.bone, a.offset))
                .collect::<Vec<_>>(),
            before
        );
    }
    Ok(())
}

fn load(files: &Files, monster: &Monster, variant: usize) -> Result<Battle> {
    let (actor, model) = battle::model::enemy(files, monster, variant, setup(7))?;
    Ok(Battle::new(Arc::new(PreparedBattle::new(
        vec![actor],
        vec![],
        1,
        vec![Some(model)],
        vec![],
    )?)))
}

#[test]
fn enemy_variants_share_rig_and_clips_and_keep_contact_radii_unscaled() -> Result<()> {
    let (files, monster) = snapshot()?;
    for (variant, expected) in [(0, (500, 90)), (1, (120, 20))] {
        let mut active = load(&files, &monster, variant)?;
        let actor = &active.actors()[0];
        assert_eq!((actor.hp, actor.tp), expected);
        assert_eq!(actor.guard.break_pressure, 999); // Enemy template, not party max-HP formula.
        assert_eq!(actor.elements.base, monster.attack_element);
        assert_eq!(
            actor.affinities,
            [
                Affinity::Normal,
                Affinity::Weak,
                Affinity::Resistant,
                Affinity::Absorb,
                Affinity::Immune,
                Affinity::Normal,
                Affinity::Normal,
                Affinity::Normal,
                Affinity::Normal
            ]
        );
        assert_eq!(actor.body.scale, 1.5);
        assert_eq!(actor.body.points[0].radius, 80.);
        assert_eq!(actor.body.points[0].center, [0., 0., -15.]);
        assert_eq!(actor.body.approach_points, actor.body.points);
        let frame = active.step(BattleInput::default())?;
        assert_eq!(
            (
                frame.models[0].resource,
                frame.models[0].clip,
                frame.models[0].frame
            ),
            (7, 0, 1.)
        );
    }
    let mut missing = monster.clone();
    missing.preview.parts[0]
        .scene
        .clips
        .retain(|clip| clip.resource_slot != 4);
    let (_, model) = battle::model::enemy(&files, &missing, 0, setup(7))?;
    assert_eq!(model.hurt_motions, [Some(3), Some(3)]);
    assert_eq!(model.guard_motions, [Some(2), None]);
    Ok(())
}

#[test]
fn enemy_guard_recovery_resolves_resource_preference_before_kind_zero_default() -> Result<()> {
    let (mut files, monster) = snapshot()?;
    let path = battle_model::enemy_path(monster.id);
    let mut source: Enemy = files.json(&path)?;
    for (preference, bonus) in [(0, 10), (1, 0), (2, 5), (3, 25), (6, 10)] {
        source.guard_preference = preference;
        files
            .bytes
            .insert(path.clone(), serde_json::to_vec(&source)?.into());
        let (actor, _) = battle::model::enemy(&files, &monster, 0, setup(7))?;
        assert_eq!(actor.guard.recovery_bonus, bonus, "preference {preference}");
        assert_eq!(actor.guard.auto_chance, 0); // Resolved metadata consumes no RNG.
    }
    Ok(())
}

#[test]
fn invalid_enemy_resources_cannot_replace_the_active_generation() -> Result<()> {
    let (files, monster) = snapshot()?;
    let mut active = load(&files, &monster, 0)?;
    let original = active.actors().to_vec();
    let path = battle_model::enemy_path(monster.id);
    let source: Enemy = files.json(&path)?;
    for change in [
        |s: &mut Enemy| s.body.transform_kinds[1] = 2,
        |s: &mut Enemy| s.body.skeleton.bones[1].name = "different".into(),
        |s: &mut Enemy| s.body.skeleton.bones[1].parent = Some(1),
        |s: &mut Enemy| s.body.volumes[0].bone = 99,
        |s: &mut Enemy| s.body.volumes[0].radius = -1.,
    ] {
        let mut changed = source.clone();
        change(&mut changed);
        let mut candidate = files.clone();
        candidate
            .bytes
            .insert(path.clone(), serde_json::to_vec(&changed)?.into());
        assert!(load(&candidate, &monster, 0).is_err());
    }
    for path in [&path, &monster.preview.parts[0].scene.clips[0].motion] {
        let mut candidate = files.clone();
        candidate.bytes.remove(path);
        assert!(load(&candidate, &monster, 0).is_err());
    }
    let mut duplicate = monster.clone();
    duplicate.preview.parts[0]
        .scene
        .clips
        .push(monster.preview.parts[0].scene.clips[0].clone());
    assert!(load(&files, &duplicate, 0).is_err());
    assert!(load(&files, &monster, 2).is_err());
    active.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(active.actors(), original);
    Ok(())
}

#[test]
fn party_body_preparation_keeps_session_state_and_requires_its_own_battle_bank() -> Result<()> {
    let (mut files, monster) = snapshot()?;
    let enemy: Enemy = files.json(&battle_model::enemy_path(monster.id))?;
    files.bytes.insert(
        battle_model::party_path(1),
        serde_json::to_vec(&battle_model::Party {
            body_sha256: "b".repeat(64),
            motion_sha256: "c".repeat(64),
            body: enemy.body,
            parts: vec![monster.preview.parts[0].scene.clone()],
            files: enemy.files,
        })?
        .into(),
    );
    files.bytes.insert(
        resonance_content::battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&resonance_content::battle_profile::Table {
            default_strategy: [[0; 3]; 10],
            companion_policy: Default::default(),
            placement: Default::default(),
            entry: Default::default(),
            chant: vec![],
            voice_sequences: vec![vec![]; 10],
            death_voice_pairs: vec![],
            contact_sounds: Default::default(),
            source_sha256: "a".repeat(64),
            records: vec![enemy.profile; 11],
        })?
        .into(),
    );
    let mut session = actor();
    session.max_hp = 328;
    session.stats.slash = 122;
    session.control = resonance_battle::Control::SemiAuto;
    let (ready, model) = battle::model::party(&files, 1, session.clone(), setup(7))?;
    assert_eq!(
        (ready.hp, ready.tp, ready.stats, ready.control),
        (session.hp, session.tp, session.stats, session.control)
    );
    assert_eq!(ready.guard.break_pressure, 6);
    assert_eq!(model.hurt_bones, [1]);
    assert_eq!(model.hurt_motions, [Some(3), Some(4)]);
    let active = Battle::new(Arc::new(PreparedBattle::new(
        vec![ready],
        vec![],
        1,
        vec![Some(model)],
        vec![],
    )?));
    assert_eq!(active.actors()[0].body.points[0].radius, 80.);
    for character in [0, 2, 255] {
        assert!(battle::model::party(&files, character, session.clone(), setup(7)).is_err());
    }
    let mut wrong_side = session.clone();
    wrong_side.side = Side::Enemy;
    assert!(battle::model::party(&files, 1, wrong_side, setup(7)).is_err());
    files.bytes.remove("motion/0.motion");
    assert!(battle::model::party(&files, 1, session, setup(7)).is_err());
    assert_eq!(active.actors()[0].hp, 10);
    Ok(())
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cold_party_bodies_bind_original_battle_clips_and_contact_bones() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    for character in 1..=9 {
        let files = battle::model::load_files(
            &root,
            field.clone(),
            &[ModelSource::Party(character)],
            &mut cache,
            || false,
        )?;
        let source: battle_model::Party = files.json(&battle_model::party_path(character))?;
        let (ready, model) =
            battle::model::party(&files, character, actor(), setup(u32::from(character)))?;
        assert_eq!(ready.guard.break_pressure, 4);
        assert!(!ready.body.points.is_empty());
        assert!(
            model.motions.contains_key(&30),
            "character {character} lacks its first normal-attack clip"
        );
        if matches!(character, 3 | 5) {
            for clip in [8, 12] {
                assert!(
                    model.motions.contains_key(&clip),
                    "caster {character} clip {clip}"
                );
            }
        }
        assert_eq!(source.body.skeleton.bones.len(), model.skeleton.bones.len());
        let mut active = Battle::new(Arc::new(PreparedBattle::new(
            vec![ready],
            vec![],
            1,
            vec![Some(model)],
            vec![],
        )?));
        let frame = active.step(BattleInput::default())?;
        assert_eq!(frame.models[0].resource, u32::from(character));
        assert_eq!(frame.models[0].frame, 1.);
        assert!(
            frame.actors[0]
                .body
                .points
                .iter()
                .all(|point| point.center.iter().all(|v| v.is_finite()))
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cold_opening_enemy_bodies_use_shared_clips_and_match_observed_statistics() -> Result<()> {
    let files = Files::load(
        &common::asset_root(),
        &["fields/map-340.preload.json"],
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let files = battle::model::load_files(
        &common::asset_root(),
        files,
        &[ModelSource::Enemy(49), ModelSource::Enemy(36)],
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let monster = &menu.monsters.records[49];
    let bank: resonance_content::battle_effect::SourceBank =
        files.json(resonance_content::battle_effect::COMMON_PATH)?;
    let (actor, model) = battle::model::enemy(
        &files,
        monster,
        1,
        ModelSetup {
            stun: Some(battle::model::StunResources {
                particle: Arc::new(resonance_battle::ParticleDefinition {
                    model: None,
                    resource: 8,
                    member: 19,
                    data: bank.particle(19)?,
                }),
                sound: resonance_battle::SoundBinding {
                    resource: 9,
                    index: 117,
                },
            }),
            ..setup(7)
        },
    )?;
    assert_eq!(model.skeleton.bones.len(), 34);
    assert_eq!(model.hurt_bones, [3]);
    assert_eq!(model.stun.as_ref().context("missing stun")?.head, 5);
    assert_eq!(model.hurt_motions, [Some(3), Some(4)]);
    assert_eq!(model.guard_motions, [Some(2), Some(4)]);
    assert_eq!(model.knockdown.context("missing knockdown")?.down_motion, 7);
    assert_eq!(actor.hp, 320);
    assert_eq!(actor.body.points[0].radius, 80.);
    assert_eq!(actor.affinities[0], Affinity::Resistant);
    assert_eq!(actor.affinities[7], Affinity::Weak);
    let (zombie, zombie_model) =
        battle::model::enemy(&files, &menu.monsters.records[36], 0, setup(8))?;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-guard.json"
    ))?;
    for row in fixture["observations"]
        .as_array()
        .context("missing observations")?
    {
        let observed = &row["target"];
        // The seventh resolver call targets the other enemy in formation 2.
        let actor = if row["index"] == 6 { &zombie } else { &actor };
        let s = actor.stats;
        assert_eq!(
            serde_json::json!({ "slash":s.slash, "thrust":s.thrust, "defense":s.defense,
            "intelligence":s.intelligence, "accuracy":s.accuracy, "evasion":s.evasion, "level":s.level }),
            observed["stats"]
        );
        assert_eq!(
            actor.max_hp,
            observed["max_hp"].as_i64().context("missing HP")? as i32
        );
        assert_eq!(
            actor.guard.break_pressure,
            observed["guard"]["break_pressure"]
                .as_i64()
                .context("missing guard limit")? as i16
        );
        assert_eq!(
            actor.guard.reduction,
            observed["guard"]["reduction"]
                .as_u64()
                .context("missing guard reduction")? as u8
        );
        assert_eq!(
            actor.guard.allow_airborne,
            observed["guard"]["allow_airborne"]
                .as_bool()
                .context("missing flight")?
        );
    }
    let mut active = Battle::new(Arc::new(PreparedBattle::new(
        vec![actor, zombie],
        vec![],
        1,
        vec![Some(model), Some(zombie_model)],
        vec![],
    )?));
    let frame = active.step(BattleInput::default())?;
    assert_eq!(frame.models[0].bones.len(), 34);
    assert!(
        active.actors()[0].body.points[0]
            .center
            .iter()
            .all(|v| v.is_finite())
    );
    Ok(())
}

#[test]
#[ignore = "requires current cooked party and owner-linked weapon publications; no devices"]
fn cold_genis_weapon_uses_original_owner_bank_and_separate_sampled_pose() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        field,
        &[ModelSource::Party(3), ModelSource::Weapon(175)],
        &mut cache,
        || false,
    )?;
    let body: battle_model::Party = files.json(&battle_model::party_path(3))?;
    let bank: battle_model::Weapons = files.json(battle_model::WEAPONS_PATH)?;
    assert_eq!(bank.owner_motion_sha256, body.motion_sha256);
    let weapon = battle::model::weapon(&files, 175)?;
    let part = &weapon.parts[&0];
    assert_eq!(part.rig.skeleton.bones.len(), 15);
    assert_eq!(
        part.layers[0]
            .scene
            .clips
            .iter()
            .map(|clip| clip.resource_slot)
            .collect::<Vec<_>>(),
        [60, 90, 91, 92, 103, 105]
    );
    let mut setup = setup(3);
    setup.initial.frame = 9.;
    let (session, mut model) = battle::model::party(&files, 3, actor(), setup)?;
    let groups = battle::weapon::attach(
        &files,
        Arc::make_mut(&mut model),
        part,
        0,
        body.body.attachments[&0],
        30,
        battle::weapon::owner_linked(),
    )?;
    let prepared = Arc::new(PreparedBattle::new(
        vec![session],
        vec![],
        1,
        vec![Some(model)],
        vec![],
    )?);
    let mut active = Battle::new(prepared);
    let frame = active.step(BattleInput::default())?;
    assert_eq!(frame.models[0].frame, 10.);
    assert_eq!(frame.weapons[0].clip, Some(60));
    assert_eq!(frame.weapons[0].frame, 1.);
    assert_eq!(
        frame.weapons[0].links,
        (0..9).map(|bone| [bone, bone + 1]).collect::<Vec<_>>()
    );
    let shown = &frame.weapons[0];
    let clip = part.layers[0]
        .scene
        .clips
        .iter()
        .find(|clip| clip.resource_slot == 60)
        .unwrap();
    let motion = Motion::decode(&files.read(&clip.motion)?)?;
    assert!(motion.tracks.iter().all(|track| track.bind_channels
        == part.rig.skeleton.bones[usize::from(track.bone)].bind_channels));
    let pose = part.rig.skeleton.sample(&motion, shown.frame)?;
    for (actual, expected) in shown.bones.iter().zip(&pose.global) {
        for (actual, expected) in actual.iter().flatten().zip(expected.iter().flatten()) {
            assert!((actual - expected).abs() < 0.001);
        }
    }
    for (&anchor, &bone) in groups[0].iter().zip(&part.rig.attack_groups[&0]) {
        let expected =
            resonance_content::animation::transform_point(shown.world, pose.point(bone, [0.; 3])?);
        assert_eq!(frame.actors[0].body.anchors[usize::from(anchor)], expected);
    }
    Ok(())
}
