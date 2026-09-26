use super::*;
use anyhow::Context;
use resonance_content::{battle_profile, source::FloatOperand};

pub(super) fn profile() -> battle_profile::Profile {
    battle_profile::Profile {
        walk_speed: FloatOperand::Value(5.),
        run_speed: FloatOperand::Value(10.),
        turn_ticks: 8,
        idle_ticks: 0,
        idle_variation: 0,
        initial_motion: 0,
        initial_motion_override: 0,
        texture_channels: vec![],
        idle_expression: [0; 4],
        weapon_draw_flags: vec![],
        condition_flags: [0; 2],
        condition_immunity: [0x100, 4],
        intrinsic_conditions: [0; 2],
        weight: 2,
        stun_resistance: 35,
        stagger_threshold: 100,
        stagger_ticks: 45,
        guard_reduction: 75,
        guard_pressure_limit: 999,
        flags: 0x100401,
        body_flags: 0x4000,
        armor: 3,
        center_offset: [
            FloatOperand::Value(10.),
            FloatOperand::Value(80.),
            FloatOperand::Value(-5.),
        ],
        target_bone: 0,
        target_offset: [FloatOperand::Value(0.); 3],
        model_scale: FloatOperand::Value(1.5),
        shadow_scale: FloatOperand::Value(1.),
        shadow_color: [0; 4],
        effect_scale: FloatOperand::Value(0.9),
        camera_yaw_offset: 30,
        camera_category: 3,
        camera_minimum_radius: 2800,
        voice_base: 1,
        death_voice: 0,
        death_motion: 0,
        overlimit_gain: 0,
        ground_offset: FloatOperand::Value(50.),
        head_bone: 6,
        stun_offset: [FloatOperand::Value(0.); 3],
        casting: battle_profile::Casting {
            base_ticks: 90,
            loop_start: 0,
            animation_rate: FloatOperand::Value(0.5),
            command_index: -1,
            effect_interval: 8,
            motion_flags: 0,
            resume_start: 0,
            resume_blend: 4,
            resume_loop_start: 10,
            stored_recovery_clip: 0,
        },
        storage: vec![],
    }
}

fn snapshot(profile: battle_profile::Profile) -> Result<Files> {
    let mut files = files("");
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&battle_profile::Table {
            default_strategy: [[0; 3]; 10],
            companion_policy: Default::default(),
            placement: Default::default(),
            entry: Default::default(),
            chant: vec![],
            voice_sequences: vec![vec![]; 10],
            death_voice_pairs: vec![],
            contact_sounds: Default::default(),
            source_sha256: "a".repeat(64),
            records: vec![profile; 11],
        })?
        .into(),
    );
    Ok(files)
}

#[test]
fn party_templates_use_prepared_hp_and_preserve_session_statistics() -> Result<()> {
    let files = snapshot(profile())?;
    for max_hp in [1, 99, 100, 328, 9999, 3_276_500] {
        let mut session = actor();
        session.max_hp = max_hp;
        session.hp = max_hp;
        session.stats.slash = 321;
        let ready = battle::profile::party(&files, 1, session.clone())?;
        assert_eq!(ready.guard.break_pressure, (max_hp / 100 + 3) as i16);
        assert_eq!(ready.guard.reduction, 75);
        assert!(ready.guard.allow_airborne);
        assert!(!ready.guard.auto_disabled);
        assert_eq!(ready.hp, session.hp);
        assert_eq!(ready.tp, session.tp);
        assert_eq!(ready.stats, session.stats);
        assert_eq!(ready.control, session.control);
        assert_eq!(ready.body.scale, 1.5);
        assert_eq!(ready.effect_scale, 0.9);
        assert_eq!(
            ready.framing,
            resonance_battle::ActorFraming {
                yaw_offset: 30.,
                large: true,
                minimum_radius: 2800.,
            }
        );
        assert_eq!(ready.body.center_offset, [10., 80., -5.]);
        assert_eq!(ready.movement.hover_height, 50.);
        assert!(ready.movement.flying && ready.movement.fixed_height);
        assert!(ready.reaction.recover_in_air);
        assert_eq!(
            ready.reaction.profile.vertical,
            resonance_battle::VerticalRecoil::Scale(0.75)
        );
        assert!(ready.reaction.profile.clear_pending);
        assert_eq!(
            (ready.reaction.armor.base, ready.reaction.armor.threshold),
            (3, 3)
        );
        assert_eq!(
            (
                ready.reaction.stagger.threshold,
                ready.reaction.stagger.duration
            ),
            (100, 45)
        );
        assert_eq!(ready.reaction.stun.resistance, 35);
        assert!(ready.reaction.stun.immune && ready.reaction.stun.shortened);
    }
    let mut changed = profile();
    changed.flags = 0x4000;
    changed.condition_immunity = [0; 2];
    let ready = battle::profile::party(&snapshot(changed)?, 11, actor())?;
    assert!(ready.guard.auto_disabled);
    assert!(!ready.guard.allow_airborne && !ready.reaction.recover_in_air);
    assert!(!ready.reaction.stun.immune && !ready.reaction.stun.shortened);
    Ok(())
}

#[test]
fn invalid_party_preparation_cannot_replace_the_active_generation() -> Result<()> {
    let files = snapshot(profile())?;
    let ready = battle::profile::party(&files, 1, actor())?;
    let mut active = Battle::new(Arc::new(resonance_battle::PreparedBattle::new(
        vec![ready.clone()],
        vec![],
        1,
        vec![],
        vec![],
    )?));
    let before = active.actors().to_vec();
    for character in [0, 12, 255] {
        assert!(battle::profile::party(&files, character, actor()).is_err());
    }
    let mut enemy = actor();
    enemy.side = Side::Enemy;
    assert!(battle::profile::party(&files, 1, enemy).is_err());
    assert!(battle::profile::party(&Files::default(), 1, actor()).is_err());
    let mut changed = profile();
    changed.model_scale = FloatOperand::Bits { bits: 0x7fc12345 };
    assert!(battle::profile::party(&snapshot(changed)?, 1, actor()).is_err());
    let mut changed = profile();
    changed.center_offset[1] = FloatOperand::Bits { bits: 0xff800000 };
    assert!(battle::profile::party(&snapshot(changed)?, 1, actor()).is_err());
    // The caster and model binders validate their own operands when consumed.
    let mut changed = profile();
    changed.casting.animation_rate = FloatOperand::Bits { bits: 0x7fc12345 };
    assert!(battle::profile::party(&snapshot(changed)?, 1, actor()).is_ok());
    active.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(active.actors(), before);
    Ok(())
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cold_party_profiles_match_observed_opening_lloyd_traits() -> Result<()> {
    let files = Files::load(
        &common::asset_root(),
        &["fields/map-340.preload.json"],
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let table: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    assert_eq!(table.records.len(), 11);
    let recoil: resonance_content::battle_recoil::Table =
        files.json(resonance_content::battle_recoil::PATH)?;
    assert_eq!(table.source_sha256, recoil.source_sha256);
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-guard.json"
    ))?;
    for row in fixture["observations"]
        .as_array()
        .context("missing observations")?
    {
        let original = &row["owner"];
        let mut session = actor();
        session.hp = serde_json::from_value(original["hp"].clone())?;
        session.max_hp = serde_json::from_value(original["max_hp"].clone())?;
        let prepared = battle::profile::party(
            &files,
            serde_json::from_value(original["character"].clone())?,
            session,
        )?;
        let p = &original["profile"];
        assert_eq!(
            serde_json::json!({
                "guard_pressure_limit": prepared.guard.break_pressure,
                "guard_reduction": prepared.guard.reduction,
                "stagger_threshold": prepared.reaction.stagger.threshold,
                "stagger_ticks": prepared.reaction.stagger.duration,
                "stun_resistance": prepared.reaction.stun.resistance,
                "stun_immune": prepared.reaction.stun.immune,
                "stun_shortened": prepared.reaction.stun.shortened,
                "scale_bits": prepared.body.scale.to_bits(),
                "hover_height_bits": prepared.movement.hover_height.to_bits(),
                "flying": prepared.movement.flying,
                "fixed_height": prepared.movement.fixed_height,
                "recover_in_air": prepared.reaction.recover_in_air,
                "auto_guard_disabled": prepared.guard.auto_disabled,
                "air_guard": prepared.guard.allow_airborne,
            }),
            *p
        );
    }
    Ok(())
}
