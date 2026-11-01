use super::*;
use resonance_battle::conditions::{Condition::*, ConditionSet};
use resonance_content::battle_profile;

pub(super) fn profile() -> battle_profile::Profile {
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
        immunities: ConditionSet::of(&[Stun, ShortStun]),
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

#[test]
fn party_templates_use_prepared_hp_and_preserve_session_statistics() -> Result<()> {
    let files = files();
    let recoil = battle::recoil::Parameters::load(&files)?;
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
        battle::profile::apply(&recoil, &template, &mut ready)?;
        assert_eq!(ready.guard.break_pressure, break_pressure);
        assert_eq!(ready.guard.reduction, 75);
        assert!(ready.guard.allow_airborne);
        assert!(!ready.guard.auto_disabled);
        assert_eq!(ready.hp, session.hp);
        assert_eq!(ready.tp, session.tp);
        assert_eq!(ready.equipment.stats, session.equipment.stats);
        assert_eq!(ready.control, session.control);
        assert!(ready.reaction.recover_in_air);
        assert_eq!(
            ready.reaction.profile.vertical,
            resonance_battle::VerticalRecoil::Scale(0.75)
        );
        assert!(ready.reaction.profile.clear_pending);
    }
    let mut changed = profile();
    changed.traits = battle_profile::ProfileTraits {
        auto_guard_disabled: true,
        fixed_height: true,
        ..Default::default()
    };
    let mut ready = actor();
    battle::profile::apply(&recoil, &changed, &mut ready)?;
    assert!(ready.guard.auto_disabled);
    assert!(!ready.guard.allow_airborne && !ready.reaction.recover_in_air);
    Ok(())
}

#[test]
fn authored_information_is_applied_without_clearing_learned_knowledge() -> Result<()> {
    let recoil = battle::recoil::Parameters::load(&files())?;
    let mut template = profile();
    template.traits.initial_weakness_known = true;
    let mut ready = actor();
    ready.resistance_known = true;
    battle::profile::apply(&recoil, &template, &mut ready)?;
    assert!(ready.weakness_known && ready.resistance_known);
    template.traits.initial_weakness_known = false;
    battle::profile::apply(&recoil, &template, &mut ready)?;
    assert!(ready.weakness_known && ready.resistance_known);
    Ok(())
}

#[test]
fn condition_profiles_compose_saved_and_gear_layers_but_cannot_freeze_actors() -> Result<()> {
    use resonance_battle::conditions::{Conditions, Layers};
    let mut template = profile();
    template.initial_conditions = ConditionSet::of(&[AttackUp, Weak]);
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
    let layers = owner.conditions.layers();
    assert_eq!(layers.base, ConditionSet::of(&[PoisonMild, AttackUp, Weak]));
    assert_eq!(layers.intrinsic, AilmentResistance.into());
    assert!(layers.equipment_overlay.is_empty());
    assert_eq!(
        owner.conditions.effective(),
        ConditionSet::of(&[PoisonMild, AttackUp, Weak, CastingSpeed, AilmentResistance])
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
