use super::*;
use crate::conditions::ConditionSet;
use resonance_content::battle_action::Condition as HitAilment;
#[path = "all_divide_tests.rs"]
mod all_divide_tests;
#[path = "damage_modifier_tests.rs"]
mod damage_modifier_tests;
#[path = "kill_recovery_tests.rs"]
mod kill_recovery_tests;
#[path = "unison_tests.rs"]
mod unison_tests;
use crate::{
    Activity, Affinity, Battle, Control, DamageKind, GuardResult, GuardRule, HitElement, Power,
    ProjectileDefinition,
};
use std::sync::Arc;

fn rule(power: u16) -> HitRule {
    HitRule {
        sound: None,
        effect_on_guard: false,
        effect: None,
        overlimit_pause: true,
        kind: DamageKind::Slash,
        arte: true,
        power: Power::Fixed(power),
        element: HitElement::Neutral,
        prevents_defeat: false,
        finisher_grade: false,
        hp_drain: false,
        reaction: Default::default(),
        guard: GuardRule {
            pressure: 0,
            ..Default::default()
        },
        condition: None,
    }
}

fn strike(battle: &mut Battle, rule: HitRule) -> Result<Vec<Cue>> {
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Melee {
            actor: ActorId(0),
            action: ActionId(1),
        },
        owner: ActorId(0),
        origin: Origin::World([0.; 3]),
        radius: 20.,
        height: 20.,
        shape: HitShape::Box,
        rule,
        clashes: false,
        struck: Vec::new(),
    })?;
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues, &[])?;
    Ok(cues)
}

#[test]
fn contact_uses_the_first_overlapping_marker_once_for_damage_and_feedback() -> Result<()> {
    use resonance_content::animation::{Bone, Transform, TransformChannels};

    for target_side in [Side::Party, Side::Enemy] {
        let owner_side = if target_side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        };
        let mut owner = crate::tests::actor(owner_side);
        owner.position = [100., 0., 0.];
        let mut target = crate::tests::actor(target_side);
        target.body.scale = 2.;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let hp = target.hp;
        let mut battle = crate::tests::prepared(vec![owner, target], 1).finish()?;
        let mut owner_model = crate::tests::model_definition(8, [(0, 1.)]);
        owner_model.skeleton.bones.push(Bone {
            name: "attack".into(),
            parent: Some(0),
            bind_channels: TransformChannels(8),
            bind: Transform {
                translation: [96., 0., 0.],
                ..Default::default()
            },
        });
        let mut model = crate::tests::model_definition(7, [(0, 1.)]);
        model.skeleton.bones.push(Bone {
            name: "dm5_target".into(),
            parent: Some(0),
            bind_channels: TransformChannels(8),
            bind: Transform {
                translation: [50., 0., 0.],
                ..Default::default()
            },
        });
        model.reception_spheres = Some(vec![
            crate::BodySphere {
                bone: 0,
                radius: 1.,
            },
            crate::BodySphere {
                bone: 1,
                radius: 10.,
            },
            crate::BodySphere {
                bone: 1,
                radius: 15.,
            },
        ]);
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![Some(Arc::new(owner_model)), Some(Arc::new(model))],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        let mut contacts = Contacts::default();
        contacts.push(Contact {
            source: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1),
            },
            owner: ActorId(0),
            origin: Origin::Bone(crate::MeleeAnchor {
                resource: 8,
                bone: 1,
            }),
            radius: 0.,
            height: 0.,
            shape: HitShape::Sphere,
            rule: rule(10),
            clashes: false,
            struck: Vec::new(),
        })?;
        let mut cues = Vec::new();
        // The attachment uses its current pose with the placement before movement.
        let placements = [Some(glam::Mat4::IDENTITY.to_cols_array_2d())];
        contacts.resolve(&mut battle, &mut cues, &placements)?;
        assert_eq!(battle.actors[1].hp, hp - 10);
        let impacts: Vec<_> = cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Hit { position, .. } => Some(*position),
                _ => None,
            })
            .collect();
        assert_eq!(impacts, [[90., 0., 0.]]);
    }
    Ok(())
}

#[test]
fn hp_drain_uses_nominal_damage_and_requires_an_unsuppressed_hit() -> Result<()> {
    let fixture = || -> Result<Battle> {
        let mut owner = crate::tests::actor(Side::Enemy);
        owner.hp = 990;
        owner.equipment.max_hp = 1000;
        let mut target = crate::tests::actor(Side::Party);
        target.hp = 1000;
        target.equipment.max_hp = 1000;
        target.body.collider = Some(crate::Collider::sphere(1.));
        target.facing_direction = [0., 0., -1.];
        target.guard.break_pressure = 31;
        crate::tests::prepared(vec![owner, target], 1).finish()
    };
    let mut hit = rule(100);
    hit.hp_drain = true;

    // Overkill restores nominal damage, while the attacker's own HP cap limits application.
    let mut battle = fixture()?;
    battle.actors[1].hp = 25;
    let cues = strike(&mut battle, hit)?;
    assert_eq!(battle.actors[1].hp, 0);
    assert_eq!(battle.actors[0].hp, 1000);
    assert!(cues.contains(&Cue::Recovered {
        floating: true,
        actor: ActorId(0),
        kind: crate::RecoveryKind::Hp,
        nominal: 100,
        applied: 10,
    }));
    assert!(
        !cues
            .iter()
            .any(|cue| matches!(cue, Cue::EffectNotice { .. }))
    );

    let mut battle = fixture()?;
    battle.actors[1].hp = 1;
    let mut nonlethal = hit;
    nonlethal.prevents_defeat = true;
    strike(&mut battle, nonlethal)?;
    assert_eq!(battle.actors[1].hp, 1);
    assert_eq!(battle.actors[0].hp, 1000);

    for guard in [
        GuardResult::None,
        GuardResult::Blocked {
            first: true,
            special: false,
        },
        GuardResult::Broken,
    ] {
        for armored in [false, true] {
            let mut battle = fixture()?;
            let mut hit = hit;
            battle.actors[1].guard.active = guard != GuardResult::None;
            hit.guard.breaks = guard == GuardResult::Broken;
            if armored {
                battle.actors[1].reaction.protection.mode = crate::ProtectionMode::Armor;
            }
            let cues = strike(&mut battle, hit)?;
            let eligible = guard == GuardResult::None && !armored;
            assert_eq!(battle.actors[0].hp, if eligible { 1000 } else { 990 });
            assert_eq!(
                cues.iter().any(|cue| matches!(cue, Cue::Recovered { .. })),
                eligible
            );
            assert!(cues.iter().any(|cue| matches!(
                cue,
                Cue::Hit { result, .. }
                    if result.guard == if armored { GuardResult::None } else { guard }
            )));
        }
    }
    for affinity in [Affinity::Absorb, Affinity::Immune] {
        let mut battle = fixture()?;
        battle.actors[1].equipment.affinities[0] = affinity;
        strike(&mut battle, hit)?;
        assert_eq!(battle.actors[0].hp, 990);
    }
    let mut battle = fixture()?;
    let mut suppressed = hit;
    battle.actors[1].reaction.hit_shake_disabled = true;
    suppressed.reaction.suppress = true;
    strike(&mut battle, suppressed)?;
    assert_eq!(battle.actors[0].hp, 990);
    assert_eq!(battle.actors[1].reaction.hit_shake_ticks, 0);
    let mut battle = fixture()?;
    battle.actors[1].position[0] = 1000.;
    assert!(strike(&mut battle, hit)?.is_empty());
    assert_eq!(battle.actors[0].hp, 990);
    Ok(())
}

#[test]
fn post_contact_power_preserves_current_damage_guard_and_projectile_ownership() -> Result<()> {
    let fixture = |recipients| -> Result<Battle> {
        let mut owner = crate::tests::actor(Side::Enemy);
        owner.equipment.stats.slash = 1000;
        owner.equipment.stats.accuracy = 1000;
        let mut target = crate::tests::actor(Side::Party);
        target.hp = 10_000;
        target.equipment.max_hp = 10_000;
        target.body.collider = Some(crate::Collider::sphere(1.));
        target.facing_direction = [0., 0., -1.];
        target.guard.break_pressure = 31;
        crate::tests::prepared(
            std::iter::once(owner)
                .chain(std::iter::repeat_n(target, recipients))
                .collect(),
            1,
        )
        .finish()
    };
    for guarded in [false, true] {
        let mut damage = Vec::new();
        for adjustment in [0, -5, 5] {
            let mut battle = fixture(1)?;
            let mut hit = rule(0);
            hit.power = if adjustment == 0 {
                Power::Normal
            } else {
                Power::AdjustAfterContact(adjustment)
            };
            battle.actors[1].position[0] = 1000.;
            assert!(strike(&mut battle, hit)?.is_empty());
            assert_eq!(battle.actors[0].attack_power, 100);
            battle.actors[1].position[0] = 0.;
            let mut amounts = Vec::new();
            for count in 1..=2 {
                battle.actors[1].guard.active = guarded;
                let cues = strike(&mut battle, hit)?;
                let result = cues
                    .iter()
                    .find_map(|cue| match cue {
                        Cue::Hit { result, .. } => Some(result),
                        _ => None,
                    })
                    .expect("the contact must hit");
                assert_eq!(matches!(result.guard, GuardResult::Blocked { .. }), guarded);
                assert_eq!(
                    battle.actors[0].attack_power as i16,
                    100 + count * adjustment
                );
                amounts.push(result.amount);
            }
            damage.push(amounts);
        }
        assert_eq!(damage[0][0], damage[1][0]);
        assert_eq!(damage[0][0], damage[2][0]);
        assert!(damage[1][1] < damage[0][1] && damage[0][1] < damage[2][1]);
    }
    let mut battle = fixture(1)?;
    let mut definition = crate::tests::contact_projectile(false, true);
    let mut hit = rule(0);
    hit.power = Power::AdjustAfterContact(-15);
    definition.contact.as_mut().unwrap().hit = hit;
    let id = battle.emit(
        Arc::new(definition),
        ActionId(1),
        ActorId(0),
        ActorId(1),
        [0.; 3],
    )?;
    battle.actors[0].attack_power = 80;
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Projectile(id),
        owner: ActorId(0),
        origin: Origin::World([0.; 3]),
        radius: 5.,
        height: 5.,
        shape: HitShape::Sphere,
        rule: hit,
        clashes: false,
        struck: Vec::new(),
    })?;
    let mut cues = Vec::new();
    contacts.resolve(&mut battle, &mut cues, &[])?;
    assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    assert_eq!(battle.actors[0].attack_power, 65);
    assert_eq!(battle.projectiles[&id].attack_power, 100);
    let mut battle = fixture(4)?;
    for expected in [40, 10, 10] {
        let cues = strike(&mut battle, hit)?;
        assert_eq!(
            cues.iter()
                .filter(|cue| matches!(cue, Cue::Hit { .. }))
                .count(),
            4
        );
        assert_eq!(battle.actors[0].attack_power, expected);
    }
    Ok(())
}

#[test]
fn suppressed_contact_keeps_damage_and_ailments_without_guard_reaction_or_success() -> Result<()> {
    let mut hit = rule(40);
    hit.arte = false;
    hit.guard.pressure = 8;
    hit.reaction.suppress = true;
    hit.reaction.hitstun = 10;
    hit.reaction.stagger = 1;
    hit.reaction.recoil.impulse = [4., 0.];
    hit.condition = Some(crate::HitCondition {
        condition: HitAilment::PoisonMild,
        chance: 100,
        value: 0,
    });
    for (guarded, lethal) in [(false, false), (true, false), (true, true)] {
        let mut owner = crate::tests::actor(Side::Party);
        owner.tp = 0;
        let mut target = crate::tests::actor(Side::Enemy);
        target.control = Control::Manual;
        target.hp = if lethal { 1 } else { 100 };
        target.guard.active = guarded;
        target.guard.pressure = 3;
        target.guard.break_pressure = 10;
        target.body.collider = Some(crate::Collider::sphere(2.));
        if !guarded {
            let mut streams = Vec::new();
            for suppress in [false, true] {
                let mut target = target.clone();
                let mut rule = hit;
                rule.reaction.suppress = suppress;
                let mut draws = Vec::new();
                let result = damage::resolve(
                    &owner,
                    &mut target,
                    rule,
                    100,
                    [1., 0., 0.],
                    &mut |draw| {
                        draws.push(draw as u8);
                        0
                    },
                    false,
                );
                assert_eq!((result.amount, result.hp_change), (40, -40));
                assert!(
                    target
                        .conditions
                        .effective()
                        .contains(HitAilment::PoisonMild)
                );
                streams.push(draws);
            }
            assert_eq!(
                streams[0], streams[1],
                "damage and ailment draws are unchanged"
            );
        }
        let mut battle = crate::tests::prepared(vec![owner, target], 40).finish()?;
        for (actor, target, id) in [(0, 1, 1), (1, 0, 2)] {
            battle.put_sequence(
                ActionId(id),
                Box::new(crate::action::Sequence::new(
                    crate::ActionKey(0),
                    &Arc::new(crate::tests::action(40)),
                    ActorId(actor),
                    ActorId(target),
                    0,
                )),
            );
        }
        let cues = strike(&mut battle, hit)?;
        let result = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit { result, .. } => Some(*result),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.amount, 40);
        assert_eq!(result.guard, GuardResult::None);
        assert_eq!(result.reaction_suppressed, !lethal);
        assert_eq!(battle.actors[1].guard.pressure, 3);
        assert_eq!(battle.actors[1].hp, if lethal { 0 } else { 60 });
        assert_eq!(battle.actors[0].tp, u16::from(lethal));
        assert_eq!(battle.sequence(&ActionId(2)).is_some(), !lethal);
        assert!(battle.runtime[0].combo.confirmed_contact);
        assert_eq!(
            battle.sequence(&ActionId(1)).unwrap().contact_target,
            Some(ActorId(1))
        );
        assert_eq!(
            battle.sequence(&ActionId(1)).unwrap().struck_target,
            lethal.then_some(ActorId(1))
        );
        if !lethal {
            assert_eq!(battle.actors[1].reaction.combo_hits, 0);
            assert_eq!(battle.actors[1].movement.forward, 0.);
            assert_eq!(
                battle.actors[1]
                    .conditions
                    .effective()
                    .contains(HitAilment::PoisonMild),
                !guarded
            );
        }
    }
    Ok(())
}

#[test]
fn authored_contact_effect_uses_the_hit_point_and_respects_guard_permission() -> Result<()> {
    let effect = crate::EffectAppearance {
        resource: 2,
        member: 7,
    };
    let mut results = [None; 2];
    for (guarded, effect_on_guard) in [(false, false), (false, true), (true, false), (true, true)] {
        let owner = crate::tests::actor(Side::Enemy);
        let mut target = crate::tests::actor(Side::Party);
        target.heading = 180.;
        target.facing_direction = crate::control::direction_from_heading(target.heading);
        target.guard.active = guarded;
        target.guard.break_pressure = 31;
        target.body.collider = Some(crate::Collider::sphere(2.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1).finish()?;
        let cues = strike(
            &mut battle,
            HitRule {
                effect: Some(effect),
                effect_on_guard,
                sound: Some(crate::Sound::Cue(107)),
                guard: GuardRule {
                    suppress_sound: effect_on_guard,
                    ..Default::default()
                },
                ..rule(5)
            },
        )?;
        let (position, result) = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit {
                    position,
                    result,
                    contact_sound,
                    suppress_guard_sound,
                    ..
                } => {
                    assert_eq!(*contact_sound, Some(crate::Sound::Cue(107)));
                    assert_eq!(*suppress_guard_sound, effect_on_guard);
                    Some((*position, *result))
                }
                _ => None,
            })
            .expect("contact must remain visible to ordinary feedback");
        assert_eq!(matches!(result.guard, GuardResult::Blocked { .. }), guarded);
        if let Some(previous) = results[usize::from(guarded)] {
            assert_eq!(result, previous);
        } else {
            results[usize::from(guarded)] = Some(result);
        }
        let effects: Vec<_> = cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::DeferredEffect(request) => Some(request),
                _ => None,
            })
            .collect();
        assert_eq!(effects.len(), usize::from(!guarded || effect_on_guard));
        if let Some(request) = effects.first() {
            assert_eq!(request.appearance, effect);
            assert_eq!(request.origin, position);
            assert_eq!(request.heading, 180.);
            assert_eq!((request.owner, request.target), (ActorId(0), ActorId(1)));
            assert!(request.follow.is_none());
        }
    }
    Ok(())
}

fn combo_grade_contact(
    victim: Side,
    prior_hits: i32,
    hp: i32,
    guarded: bool,
) -> Result<(Battle, Vec<Cue>)> {
    let owner_side = if victim == Side::Enemy {
        Side::Party
    } else {
        Side::Enemy
    };
    let owner = crate::tests::actor(owner_side);
    let mut target = crate::tests::actor(victim);
    target.hp = hp;
    target.equipment.max_hp = 1000;
    target.heading = 180.;
    target.facing_direction = crate::control::direction_from_heading(target.heading);
    target.guard.active = guarded;
    target.guard.break_pressure = 31;
    target.reaction.combo_hits = prior_hits;
    target.reaction.combo_damage = 210;
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = crate::PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        Default::default(),
        1,
    )?
    .finish()?;
    let cues = strike(&mut battle, rule(54))?;
    Ok((battle, cues))
}

#[test]
fn native_melee_volume_turns_scales_and_follows_the_owner_without_a_model() -> Result<()> {
    let mut owner = crate::tests::actor(Side::Party);
    owner.position = [10., 0., 20.];
    owner.heading = 90.;
    owner.body.scale = 2.;
    let mut front = crate::tests::actor(Side::Enemy);
    front.position = [90., 60., 20.];
    front.body.collider = Some(crate::Collider::sphere(2.));
    let mut behind = front.clone();
    behind.position[0] = -70.;
    let definition = MeleeDefinition {
        hit: HitRule {
            overlimit_pause: false,
            ..rule(5)
        },
        trail: None,
        volume: crate::MeleeVolume::Fixed {
            offset: [0., 30., 40.],
            radius: 10.,
            half_height: 10.,
        },
    };
    definition.validate()?;
    let mut battle =
        crate::tests::prepared(vec![owner.clone(), front.clone(), behind, owner, front], 1)
            .finish()?;
    let mut contacts = Contacts::default();
    contacts.melee(ActorId(0), ActionId(1), &definition, &[])?;
    contacts.melee(ActorId(3), ActionId(2), &definition, &[])?;
    let previous: Vec<_> = battle.actors.iter().map(|actor| actor.position).collect();
    for actor in &mut battle.actors {
        actor.position[0] += 100.;
    }
    battle.push_bodies(&previous, &[100.; 5], &[]);
    for (actual, expected) in contacts.0[0]
        .position(&battle)?
        .into_iter()
        .zip([190., 60., 20.])
    {
        assert!((actual - expected).abs() < 0.001);
    }
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues, &[])?;
    assert_eq!(battle.actors[1].hp, 45, "one contact per victim per update");
    assert_eq!(
        battle.actors[4].hp, 45,
        "separate melee markers can hit different victims"
    );
    assert_eq!(battle.actors[2].hp, 50);
    contacts.melee(ActorId(3), ActionId(2), &definition, &[])?;
    contacts.resolve(&mut battle, &mut Vec::new(), &[])?;
    assert_eq!(
        battle.actors[1].hp, 40,
        "the skipped attack remains eligible"
    );
    assert_eq!(
        battle.actors[4].hp, 45,
        "one marker accepts only its first eligible victim"
    );
    let mut invalid = definition.clone();
    let crate::MeleeVolume::Fixed { offset, .. } = &mut invalid.volume else {
        unreachable!()
    };
    offset[0] = f32::NAN;
    assert!(invalid.validate().is_err());

    for (height, published, expected_hp) in [
        (140., false, 45),
        (540., false, 50),
        (140., true, 45),
        (540., true, 50),
    ] {
        let mut owner = crate::tests::actor(Side::Enemy);
        owner.body.center_offset = [10., height, 20.];
        owner.body.scale = 2.;
        owner.heading = 90.;
        let mut target = crate::tests::actor(Side::Party);
        target.position = [40., 80., -20.];
        target.body.collider = Some(crate::Collider::sphere(30.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1).finish()?;
        let definition = MeleeDefinition {
            hit: rule(5),
            trail: None,
            volume: crate::MeleeVolume::BodyCenter {
                shape: HitShape::Sphere,
                radius: 120.,
                half_height: 120.,
            },
        };
        definition.validate()?;
        let mut contacts = Contacts::default();
        contacts.melee(ActorId(0), ActionId(1), &definition, &[])?;
        let placement = published.then_some([
            [0., 0., -2., 0.],
            [-2., 0., 0., 0.],
            [0., 2., 0., 0.],
            [0., 0., 0., 1.],
        ]);
        if published {
            battle.actors[0].position[1] += 1000.;
            assert_eq!(
                contacts.0[0].position_at(&battle, placement)?,
                [40., height * 2., -20.],
            );
        }
        contacts.resolve(&mut battle, &mut vec![], &[placement])?;
        assert_eq!(battle.actors[1].hp, expected_hp);
    }
    Ok(())
}

#[test]
fn hit_condition_label_matches_the_condition_and_application_pose() -> Result<()> {
    for (condition, expected_label, duration) in [
        (
            HitAilment::Weak,
            crate::conditions::ConditionLabel::StatusDown,
            None,
        ),
        (
            HitAilment::Curse,
            crate::conditions::ConditionLabel::Applied,
            None,
        ),
        (
            HitAilment::Paralysis,
            crate::conditions::ConditionLabel::Applied,
            Some(600),
        ),
        (
            HitAilment::PoisonMild,
            crate::conditions::ConditionLabel::Applied,
            None,
        ),
        (
            HitAilment::PoisonSevere,
            crate::conditions::ConditionLabel::Applied,
            None,
        ),
    ] {
        let owner = crate::tests::actor(Side::Party);
        let mut target = crate::tests::actor(Side::Enemy);
        target.hp = 1000;
        target.equipment.max_hp = 1000;
        target.position = [10., 20., 30.];
        target.body.center_offset = [1., 2., 3.];
        target.body.collider = Some(crate::Collider::sphere(2.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1)
            .finish()
            .unwrap();
        let mut contacts = Contacts::default();
        contacts.push(Contact {
            source: ContactSource::Melee {
                actor: ActorId(0),
                action: ActionId(1),
            },
            owner: ActorId(0),
            origin: Origin::World(battle.actors[1].position),
            radius: 20.,
            height: 20.,
            shape: HitShape::Box,
            rule: HitRule {
                sound: None,
                effect_on_guard: false,
                effect: None,
                overlimit_pause: true,
                kind: DamageKind::Slash,
                arte: false,
                power: Power::Fixed(1),
                element: HitElement::Neutral,
                prevents_defeat: false,
                finisher_grade: false,
                hp_drain: false,
                reaction: Default::default(),
                guard: GuardRule::default(),
                condition: Some(crate::HitCondition {
                    condition,
                    chance: 100,
                    value: 0,
                }),
            },
            clashes: false,
            struck: Vec::new(),
        })?;
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues, &[])?;
        assert_eq!(battle.actors[1].conditions.effective(), condition.into());
        assert_eq!(battle.actors[1].conditions.remaining(condition), duration);
        if crate::conditions::POISON.contains(condition) {
            assert_eq!(battle.actors[1].conditions.periodic_effects().len(), 1);
        }
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::ConditionLabel {
                actor: ActorId(1),
                kind,
                ..
            } if *kind == expected_label
        )));
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::ConditionLabel {
                position: [12., 24., 36.],
                ..
            }
        )));
    }
    Ok(())
}

#[test]
fn petrified_contact_is_skipped_without_condition_or_rng() -> Result<()> {
    let owner = crate::tests::actor(Side::Party);
    let mut target = crate::tests::actor(Side::Enemy);
    target.availability = crate::ActorAvailability::Petrified;
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = crate::tests::prepared(vec![owner, target], 1)
        .finish()
        .unwrap();
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Melee {
            actor: ActorId(0),
            action: ActionId(1),
        },
        owner: ActorId(0),
        origin: Origin::World([0.; 3]),
        radius: 20.,
        height: 20.,
        shape: HitShape::Box,
        rule: HitRule {
            sound: None,
            effect_on_guard: false,
            effect: None,
            overlimit_pause: true,
            kind: DamageKind::Slash,
            arte: false,
            power: Power::Fixed(1),
            element: HitElement::Neutral,
            prevents_defeat: false,
            finisher_grade: false,
            hp_drain: false,
            reaction: Default::default(),
            guard: GuardRule::default(),
            condition: Some(crate::HitCondition {
                condition: resonance_content::battle_action::Condition::Paralysis,
                chance: 100,
                value: 0,
            }),
        },
        clashes: false,
        struck: Vec::new(),
    })?;
    let random = battle.random_state();
    let hp = battle.actors[1].hp;
    let mut cues = vec![];
    contacts.resolve(&mut battle, &mut cues, &[])?;
    assert_eq!(battle.random_state(), random);
    assert_eq!(battle.actors[1].hp, hp);
    assert_eq!(battle.actors[1].conditions.effective(), ConditionSet::EMPTY);
    assert!(!cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    assert!(
        !cues
            .iter()
            .any(|cue| matches!(cue, Cue::ConditionLabel { .. }))
    );
    Ok(())
}

#[test]
fn long_combos_keep_the_recorded_maximum_without_narrowing() -> Result<()> {
    for prior in [i32::from(u16::MAX), i32::MAX] {
        let (battle, cues) = combo_grade_contact(Side::Enemy, prior, 1000, false)?;
        assert_eq!(battle.ledger().maximum_combo, u16::MAX);
        assert!(cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
    }
    Ok(())
}

#[test]
fn accepted_combo_contacts_adjust_grade_for_each_victim_side() -> Result<()> {
    for (prior, enemy_grade, party_grade) in
        [(0, 0, -1), (3, 0, -1), (4, 2, -10), (5, 0, -1), (9, 2, -10)]
    {
        for (victim, grade) in [(Side::Enemy, enemy_grade), (Side::Party, party_grade)] {
            let (battle, cues) = combo_grade_contact(victim, prior, 1000, false)?;
            assert_eq!(battle.actors[1].reaction.combo_hits, prior + 1);
            assert_eq!(battle.ledger().grade(), grade, "{victim:?}, prior={prior}");
            assert_eq!(
                cues.iter().any(|cue| matches!(cue, Cue::Combo { .. })),
                prior > 0
            );
        }
    }
    // A blocked contact leaves the enemy's existing fifth-hit count unchanged.
    // It must not replay the combo-grade adjustment on that same count.
    let (battle, cues) = combo_grade_contact(Side::Enemy, 5, 1000, true)?;
    assert_eq!(battle.actors[1].reaction.combo_hits, 5);
    assert_eq!(battle.ledger().grade(), 0);
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Hit {
            result: crate::HitResult {
                guard: GuardResult::Blocked { .. },
                ..
            },
            ..
        }
    )));
    Ok(())
}

#[test]
fn lethal_fifth_contact_keeps_combo_grade_through_death_and_result_doubling() -> Result<()> {
    for (prior_hits, contact_grade, result_grade) in [(3, 0, 150), (4, 2, 154)] {
        let (mut battle, cues) = combo_grade_contact(Side::Enemy, prior_hits, 31, false)?;
        assert_eq!(battle.actors[1].reaction.combo_hits, 0);
        assert_eq!(battle.actors[1].reaction.combo_damage, 0);
        assert_eq!(battle.ledger().maximum_combo_damage, 264);
        assert_eq!(battle.ledger().kills, [1, 0]);
        assert_eq!(battle.ledger().last_party_killer, Some(ActorId(0)));
        assert_eq!(battle.ledger().maximum_combo, (prior_hits + 1) as u16);
        assert_eq!(battle.ledger().grade(), contact_grade);
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Combo { hits, damage: 264, .. } if *hits == prior_hits + 1
        )));
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        battle.retire_combat()?;
        battle.ledger.elapsed_ticks = 301;
        assert_eq!(
            battle.finalize_grade(&[ConditionSet::EMPTY], &[0], 0)?,
            result_grade
        );
    }
    Ok(())
}

#[test]
fn combo_feedback_includes_unselected_targets_and_automatic_allies() -> Result<()> {
    for side in [Side::Party, Side::Enemy] {
        let mut owner = crate::tests::actor(side);
        owner.control = Control::Auto;
        let mut target = crate::tests::actor(if side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        });
        target.control = Control::Auto;
        target.hp = 975;
        target.equipment.max_hp = 1000;
        target.reaction.combo_hits = 1;
        target.reaction.combo_damage = 25;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let mut other = target.clone();
        other.body.collider = None;
        if side == Side::Enemy {
            other.control = Control::Manual;
        }
        let mut battle = crate::PreparedBattle::new(
            vec![
                (owner, Default::default()),
                (target, Default::default()),
                (other, Default::default()),
            ],
            Default::default(),
            1,
        )?
        .finish()?;
        battle.runtime[0].target = ActorId(2);
        let cues = strike(&mut battle, rule(25))?;
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Combo {
                actor: ActorId(1),
                hits: 2,
                damage: 50
            }
        )));
        assert_eq!(battle.actors[1].hp, 950);
    }
    Ok(())
}

#[test]
fn defeat_commits_credit_once() -> Result<()> {
    for lethal in [false, true] {
        let owner = crate::tests::actor(Side::Party);
        let mut target = crate::tests::actor(Side::Enemy);
        target.hp = if lethal { 1 } else { 1000 };
        target.equipment.max_hp = 1000;
        target.overlimit = crate::OverLimit::new(73)?;
        target.body.collider = Some(crate::Collider::sphere(1.));

        let prepared = crate::PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            Default::default(),
            1,
        )?;
        let mut battle = prepared.finish()?;
        battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(false));
        let hit = rule(100);
        strike(&mut battle, hit)?;
        let victim = &battle.actors[1];
        assert_eq!(victim.available(), !lethal);
        assert_eq!(battle.activity(ActorId(1)) == Activity::Defeated, lethal);
        assert_eq!(victim.overlimit.charge(), if lethal { 0 } else { 73 });
        assert_eq!(battle.ledger.kills[0], u16::from(lethal));

        assert!(!battle.is_diagnostic());
        if lethal {
            assert!(strike(&mut battle, hit)?.is_empty());
            assert_eq!(battle.ledger.kills[0], 1);
        }
    }
    Ok(())
}

#[test]
fn ledger_counts_party_defeats_and_guarded_contacts() -> Result<()> {
    for guarded in [false, true] {
        let (battle, cues) =
            combo_grade_contact(Side::Party, 9, if guarded { 1000 } else { 1 }, guarded)?;
        assert!(battle.ledger().party_was_hit);
        assert_eq!(battle.ledger().kills, [0, 0]);
        assert_eq!(battle.ledger().deaths, [0, u8::from(!guarded)]);
        assert_eq!(battle.ledger().last_party_killer, None);
        assert_eq!(battle.ledger().grade(), if guarded { 0 } else { -110 });
        assert_eq!(
            cues.iter()
                .any(|cue| matches!(cue, Cue::Combo { hits: 10, .. })),
            !guarded
        );
    }
    Ok(())
}

#[test]
fn overlapping_projectiles_each_hit_once_restore_tp_and_retire() -> Result<()> {
    for matching_action in [true, false] {
        let mut owner = crate::tests::actor(Side::Party);
        owner.control = Control::Manual;
        owner.tp = 1;
        let mut target = crate::tests::actor(Side::Enemy);
        target.control = Control::Manual;
        target.hp = 100;
        target.position = [10., 0., 0.];
        target.body.collider = Some(crate::Collider::sphere(1.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1)
            .finish()
            .unwrap();
        let active_id = ActionId(if matching_action { 1 } else { 2 });
        battle.put_sequence(
            active_id,
            Box::new(crate::action::Sequence::new(
                crate::ActionKey(0),
                &Arc::new(crate::tests::action(40)),
                ActorId(0),
                ActorId(1),
                0,
            )),
        );
        let projectile = Arc::new(ProjectileDefinition {
            lifetime: Some(20),
            velocity: [2., 0., 0.],
            acceleration: [0.; 3],
            offset: [0.; 3],
            clamp_ground: true,
            active: None,
            birth: None,
            motion: Default::default(),
            effects: Default::default(),
            contact: Some(crate::ProjectileContact {
                hit: HitRule {
                    sound: None,
                    effect_on_guard: false,
                    effect: None,
                    overlimit_pause: false,
                    kind: DamageKind::Slash,
                    arte: false,
                    power: Power::Fixed(8),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    finisher_grade: false,
                    hp_drain: false,
                    guard: GuardRule::default(),
                    reaction: Default::default(),
                    condition: None,
                },
                cooldown: 20,
                repeat_limit: 0,
                radius: 1.,
                height: 2.,
                shape: HitShape::Sphere,
                offset: [0.; 3],
                radius_growth: 0.,
                height_growth: 0.,
                survives_contact: false,
                clashes: false,
            }),
        });
        for _ in 0..2 {
            battle.emit(
                Arc::clone(&projectile),
                ActionId(1),
                ActorId(0),
                ActorId(1),
                [0.; 3],
            )?;
        }
        let (mut hits, mut expired) = (0, 0);
        let mut last_hit_update = None;
        for _ in 0..25 {
            let frame = battle.step(crate::BattleInput::default())?;
            for cue in &frame.cues {
                match cue {
                    Cue::Hit {
                        actor: ActorId(1),
                        result,
                        ..
                    } => {
                        assert_ne!(last_hit_update, Some(frame.update));
                        last_hit_update = Some(frame.update);
                        hits += 1;
                        assert_eq!(result.hp_change, -8);
                    }
                    Cue::ProjectileExpired { .. } => expired += 1,
                    _ => {}
                }
            }
        }
        assert_eq!((hits, expired), (2, 2));
        assert_eq!(battle.actors[1].hp, 84);
        assert_eq!(battle.actors[0].tp, 3);
        assert!(battle.projectiles.is_empty());
        assert!(!battle.is_diagnostic());
        assert_eq!(
            battle.sequence(&active_id).unwrap().struck_target,
            matching_action.then_some(ActorId(1)),
            "old projectiles must not supply the current action's victim"
        );
    }
    Ok(())
}

#[test]
fn animated_projectile_points_share_hits_without_moving_the_projectile() -> Result<()> {
    for (active, expected_hits) in [([2, 8], 1), ([0, 50], 1), ([0, 1], 0)] {
        let mut owner = crate::tests::actor(Side::Party);
        owner.control = Control::Manual;
        owner.heading = 90.;
        let mut target = crate::tests::actor(Side::Enemy);
        target.control = Control::Manual;
        target.position = [50., 0., 0.];
        target.body.collider = Some(crate::Collider::sphere(1.));
        let mut battle = crate::tests::prepared(vec![owner, target], 1).finish()?;
        let mut definition = crate::tests::contact_projectile(false, true);
        definition.active = Some(active);
        definition.motion.contact_path = vec![
            vec![[0.; 3]],
            vec![[0.; 3]],
            // Earlier points overlap the victim; the final point is far away.
            vec![[0., 0., 50.], [0., 0., 50.], [0., 0., -50.]],
        ];
        definition.contact.as_mut().unwrap().hit.power = Power::Fixed(8);
        definition.validate()?;
        battle.emit(
            Arc::new(definition),
            ActionId(1),
            ActorId(0),
            ActorId(1),
            [0.; 3],
        )?;
        let mut hits = 0;
        for age in 0..=8 {
            let frame = battle.step(crate::BattleInput::default())?;
            assert_eq!(frame.projectiles[0].position, [0.; 3]);
            if age >= 2 {
                let points: Vec<_> = battle
                    .projectiles
                    .values()
                    .next()
                    .unwrap()
                    .contact_origins([0.; 3])
                    .collect();
                assert!((points[0][0] - 50.).abs() < 0.0001);
                assert!((points[2][0] + 50.).abs() < 0.0001);
            }
            for cue in &frame.cues {
                if let Cue::Hit {
                    actor: ActorId(1),
                    result,
                    ..
                } = cue
                {
                    assert_eq!(age, 2);
                    assert_eq!(result.hp_change, -8);
                    hits += 1;
                }
            }
        }
        assert_eq!(hits, expected_hits);
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
fn companion_hit_suppresses_selected_enemy_recoil_until_the_leaders_own_hit() -> Result<()> {
    let mut actors = [Side::Party, Side::Party, Side::Enemy].map(crate::tests::actor);
    actors[0].control = Control::SemiAuto;
    actors[1].control = Control::Auto;
    actors[0].position = [0., 0., 0.];
    actors[1].position = [0., 0., -100.];
    actors[2].position = [150., 0., 0.];
    actors[2].hp = 1000;
    actors[2].equipment.max_hp = 1000;
    let mut battle = crate::tests::prepared(actors.into(), 1).finish().unwrap();
    battle.runtime[0].target = ActorId(2);
    battle.actors[2].body.collider = Some(crate::Collider::sphere(1.));
    let previous_direction = [0., 0., -1.];
    battle.actors[2].reaction.direction = previous_direction;
    for (owner, expected_speed) in [(ActorId(1), 0.), (ActorId(0), 4.)] {
        let mut contacts = Contacts::default();
        contacts.push(Contact {
            source: ContactSource::Melee {
                actor: owner,
                action: ActionId(1),
            },
            owner,
            origin: Origin::World(battle.actors[2].position),
            radius: 2.,
            height: 2.,
            shape: HitShape::Box,
            rule: HitRule {
                sound: None,
                effect_on_guard: false,
                effect: None,
                overlimit_pause: true,
                kind: DamageKind::Slash,
                arte: true,
                power: Power::Fixed(25),
                element: HitElement::Neutral,
                prevents_defeat: false,
                finisher_grade: false,
                hp_drain: false,
                reaction: crate::ReactionRule {
                    recoil: crate::RecoilRule {
                        impulse: [4., 0.],
                        suppression_distance: 400.,
                        ..Default::default()
                    },
                    hitstun: 40,
                    ..Default::default()
                },
                guard: Default::default(),
                condition: None,
            },
            clashes: false,
            struck: Vec::new(),
        })?;
        contacts.resolve(&mut battle, &mut vec![], &[])?;
        let target = &battle.actors[2];
        assert_eq!(battle.activity(ActorId(2)), Activity::Hurt);
        assert_eq!(target.movement.forward, expected_speed);
        assert_eq!(target.reaction.recoil.pending, [expected_speed, 0.]);
        crate::tests::assert_vector_close(
            target.reaction.direction,
            if owner == ActorId(1) {
                previous_direction
            } else {
                [1., 0., 0.]
            },
            0.00001,
        );
    }
    assert_eq!(battle.actors[2].hp, 950);
    assert_eq!(battle.actors[2].reaction.combo_hits, 2);
    Ok(())
}

#[test]
fn defeat_retargets_a_surviving_opponent() -> Result<()> {
    let mut actors: Vec<_> = [Side::Party, Side::Enemy, Side::Enemy]
        .into_iter()
        .map(crate::tests::actor)
        .collect();
    actors[0].control = Control::SemiAuto;
    actors[1].hp = 1;
    actors[1].body.collider = Some(crate::Collider::sphere(1.));
    actors[2].position = [300., 0., 0.];
    let mut battle = crate::PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )?
    .finish()?;
    battle.set_decision_target(ActorId(0), ActorId(1))?;
    strike(&mut battle, rule(25))?;
    assert_eq!(battle.activity(ActorId(1)), Activity::Defeated);
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(2)));
    Ok(())
}

#[test]
fn contacts_update_gauge_and_defeat_state() -> Result<()> {
    for eligible in [false, true] {
        for (active, lethal, affinity, armored, guarded, initial, expected) in [
            (false, false, Affinity::Normal, false, false, 73, 88),
            (false, false, Affinity::Absorb, false, false, 73, 73),
            (false, false, Affinity::Immune, false, false, 73, 73),
            (false, false, Affinity::Normal, true, false, 73, 88),
            (false, false, Affinity::Normal, false, true, 73, 73),
            (false, true, Affinity::Normal, false, false, 73, 15),
            (true, true, Affinity::Normal, false, false, 1000, 15),
            (true, false, Affinity::Normal, false, false, 1000, 995),
            (true, false, Affinity::Immune, false, false, 3, 3),
        ] {
            let mut owner = crate::tests::actor(Side::Party);
            owner.tp = 17;
            let mut target = crate::tests::actor(Side::Enemy);
            target.hp = if lethal { 1 } else { 500 };
            target.equipment.max_hp = 1000;
            target.overlimit = if active {
                crate::OverLimit::active(initial).unwrap()
            } else {
                crate::OverLimit::new(initial).unwrap()
            };
            target.equipment.affinities[0] = affinity;
            target.guard.active = guarded;
            target.facing_direction = [0., 0., -1.];
            target.heading = 180.;
            target.guard.break_pressure = 20;
            target.guard.reduction = 75;
            if armored {
                target.reaction.protection.mode = crate::ProtectionMode::Armor;
            }
            target.body.collider = Some(crate::Collider::sphere(1.));
            let mut prepared = crate::PreparedBattle::new(
                vec![(owner, Default::default()), (target, Default::default())],
                Default::default(),
                1,
            )?;
            prepared.resources.actor_setup[1].overlimit_gain = 15;
            let mut battle = prepared.finish()?;
            let mut contacts = Contacts::default();
            contacts.push(Contact {
                source: ContactSource::Melee {
                    actor: ActorId(0),
                    action: ActionId(1),
                },
                owner: ActorId(0),
                origin: Origin::World([0.; 3]),
                radius: 2.,
                height: 2.,
                shape: HitShape::Sphere,
                rule: HitRule {
                    sound: None,
                    kind: DamageKind::Slash,
                    arte: true,
                    effect_on_guard: false,
                    effect: None,
                    overlimit_pause: eligible,
                    power: Power::Fixed(100),
                    element: HitElement::Neutral,
                    prevents_defeat: false,
                    finisher_grade: false,
                    hp_drain: false,
                    reaction: Default::default(),
                    guard: Default::default(),
                    condition: None,
                },
                clashes: false,
                struck: Vec::new(),
            })?;
            let mut cues = vec![];
            contacts.resolve(&mut battle, &mut cues, &[])?;
            if active && !lethal {
                assert_eq!(battle.actors[1].overlimit.remaining(), expected);
            } else {
                assert_eq!(battle.actors[1].overlimit.charge(), expected);
            }
            assert_eq!(
                battle.actors[1].overlimit.is_active(),
                active && !lethal && expected > 0
            );
            assert_eq!(battle.actors[1].available(), !lethal);
            assert_eq!(battle.ledger.kills[0], u16::from(lethal));
            assert_eq!(battle.actors[0].tp, 17); // Arte excludes TP but not tension.
            assert_eq!(battle.is_paused(), active && eligible);
            if lethal {
                assert_eq!(battle.activity(ActorId(1)), Activity::Defeated);
            }
        }
    }
    Ok(())
}

#[test]
fn ailment_ward_reports_feedback_without_erasing_critical_damage() -> Result<()> {
    let mut owner = crate::tests::actor(Side::Enemy);
    owner.equipment.damage.critical_chance_bonus = 100;
    owner.equipment.stats.slash = 100;
    let mut target = crate::tests::actor(Side::Party);
    target.hp = 1000;
    target.equipment.max_hp = 1000;
    target.conditions =
        crate::conditions::Conditions::default().with_traits(crate::conditions::Traits {
            magical_ailment_guard: true,
            ..Default::default()
        });
    target.position = [10., 20., 30.];
    target.body.center_offset = [1., 2., 3.];
    target.body.collider = Some(crate::Collider::sphere(2.));
    let mut battle = crate::tests::prepared(vec![owner, target], 0)
        .finish()
        .unwrap();
    let mut contacts = Contacts::default();
    contacts.push(Contact {
        source: ContactSource::Melee {
            actor: ActorId(0),
            action: ActionId(1),
        },
        owner: ActorId(0),
        origin: Origin::World(battle.actors[1].position),
        radius: 20.,
        height: 20.,
        shape: HitShape::Box,
        rule: HitRule {
            sound: None,
            effect_on_guard: false,
            effect: None,
            overlimit_pause: true,
            kind: DamageKind::Slash,
            arte: true,
            power: Power::Normal,
            element: HitElement::Neutral,
            prevents_defeat: false,
            finisher_grade: false,
            hp_drain: false,
            reaction: Default::default(),
            guard: GuardRule::default(),
            condition: Some(crate::HitCondition {
                condition: resonance_content::battle_action::Condition::Weak,
                chance: 100,
                value: 0,
            }),
        },
        clashes: false,
        struck: Vec::new(),
    })?;
    let mut cues = Vec::new();
    contacts.resolve(&mut battle, &mut cues, &[])?;
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::ConditionLabel {
            actor: ActorId(1),
            kind: crate::conditions::ConditionLabel::ExSkillEffect,
            position: [12., 24., 36.],
        }
    )));
    assert!(
        cues.iter().any(
            |cue| matches!(cue, Cue::Hit { actor: ActorId(1), result, .. } if result.critical)
        )
    );
    assert_eq!(battle.actors[1].conditions.base(), ConditionSet::EMPTY);
    Ok(())
}
