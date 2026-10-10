use super::*;
use resonance_battle::conditions::{Condition::*, ConditionSet};
use resonance_content::battle_profile;

pub(super) fn profile() -> battle_profile::Profile {
    battle_profile::Profile {
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
        rescue_expression: [0; 4],
        weapon_styles: Default::default(),
        initial_conditions: ConditionSet::EMPTY,
        immunities: ConditionSet::of(&[Stun, ShortStun]),
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
        model_scale: 1.5,
        shadow_scale: 1.,
        shadow_color: [0; 4],
        effect_scale: 0.9,
        camera_category: 3,
        voices: None,
        death_motion: 0,
        overlimit_gain: 0,
        initial_overlimit: 0,
        ground_offset: 50.,
        cast_ticks: 90,
    }
}

#[test]
fn party_templates_use_prepared_hp_and_preserve_session_statistics() -> Result<()> {
    let files = files();
    let template = profile();
    for (max_hp, break_pressure) in [
        (1, 3),
        (99, 3),
        (100, 4),
        (328, 6),
        (9999, 102),
        (3_276_500, 32_768),
        (i32::MAX, 21_474_839),
    ] {
        let mut session = actor();
        session.equipment.max_hp = max_hp;
        session.hp = max_hp;
        session.equipment.stats.slash = 321;
        let mut ready = session.clone();
        battle::profile::apply(
            &battle::recoil::Parameters::load(&files)?,
            &template,
            &mut ready,
        )?;
        assert_eq!(ready.guard.break_pressure, break_pressure);
        assert_eq!(ready.guard.reduction, 75);
        assert!(ready.guard.allow_airborne);
        assert!(!ready.guard.auto_disabled);
        assert_eq!(ready.hp, session.hp);
        assert_eq!(ready.tp, session.tp);
        assert_eq!(ready.equipment.stats, session.equipment.stats);
        assert_eq!(ready.species, 6);
        assert_eq!(ready.control, session.control);
        assert_eq!(ready.body.scale, 1.5);
        assert_eq!(ready.effect_scale, 0.9);
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
        assert_eq!(
            ready.conditions.immunity(),
            ConditionSet::of(&[Stun, ShortStun])
        );
    }
    let mut changed = profile();
    changed.traits = battle_profile::ProfileTraits {
        auto_guard_disabled: true,
        fixed_height: true,
        ..Default::default()
    };
    changed.immunities = ConditionSet::EMPTY;
    let mut ready = actor();
    battle::profile::apply(
        &battle::recoil::Parameters::load(&files)?,
        &changed,
        &mut ready,
    )?;
    assert!(ready.guard.auto_disabled);
    assert!(!ready.guard.allow_airborne && !ready.reaction.recover_in_air);
    assert!(ready.conditions.immunity().is_empty());
    Ok(())
}

#[test]
fn condition_profiles_compose_saved_and_gear_layers_but_cannot_freeze_actors() -> Result<()> {
    use resonance_battle::conditions::{Conditions, Layers};
    let mut template = profile();
    template.initial_conditions = AttackUp.into();
    template.intrinsic_conditions = CastingSpeed.into();
    template.immunities = Curse.into();
    let mut owner = actor();
    owner.conditions = Conditions::new(Layers {
        base: PoisonMild.into(),
        intrinsic: AilmentResistance.into(),
        immunity: Paralysis.into(),
        ..Default::default()
    });
    let files = files();
    battle::profile::apply(
        &battle::recoil::Parameters::load(&files)?,
        &template,
        &mut owner,
    )?;
    assert_eq!(
        owner.conditions.base(),
        ConditionSet::of(&[PoisonMild, AttackUp])
    );
    assert_eq!(
        owner.conditions.effective(),
        ConditionSet::of(&[PoisonMild, AttackUp, CastingSpeed, AilmentResistance])
    );
    assert_eq!(
        owner.conditions.immunity(),
        ConditionSet::of(&[Paralysis, Curse])
    );
    for intrinsic in [false, true] {
        let mut invalid = template.clone();
        if intrinsic {
            invalid.intrinsic_conditions = Petrified.into();
        } else {
            invalid.initial_conditions = Petrified.into();
        }
        let before = owner.conditions.clone();
        assert!(
            battle::profile::apply(
                &battle::recoil::Parameters::load(&files)?,
                &invalid,
                &mut owner
            )
            .is_err()
        );
        assert_eq!(owner.conditions, before);
    }
    Ok(())
}

#[test]
fn weak_profile_condition_is_loaded_as_live_base_without_saved_projection() -> Result<()> {
    let mut template = profile();
    template.initial_conditions = Weak.into();
    let mut prepared = actor();
    battle::profile::apply(
        &battle::recoil::Parameters::load(&files())?,
        &template,
        &mut prepared,
    )?;
    let layers = prepared.conditions.layers();
    assert_eq!(layers.base, Weak.into());
    assert!(layers.intrinsic.is_empty());
    assert!(layers.equipment_overlay.is_empty());
    Ok(())
}
