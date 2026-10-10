use super::*;
use crate::TargetPolicy;

fn actor(side: Side, x: f32) -> crate::Actor {
    let mut actor = crate::tests::actor(side);
    actor.control = if side == Side::Party {
        crate::Control::Auto
    } else {
        crate::Control::Enemy
    };
    actor.position = [x, 0., 0.];

    actor
}

fn choices(strategies: &[TargetPolicy]) -> Vec<EntryChoice> {
    strategies
        .iter()
        .enumerate()
        .map(|(index, &strategy)| EntryChoice {
            actor: ActorId(index as u8),
            strategy,
        })
        .collect()
}

#[test]
fn followers_observe_the_leaders_actual_target() -> Result<()> {
    let actors = vec![
        actor(Side::Party, 0.),
        actor(Side::Party, 200.),
        actor(Side::Enemy, 100.),
        actor(Side::Enemy, 300.),
    ];
    let prepared = PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        42,
    )?
    .with_entry_choices(choices(&[
        TargetPolicy::Farthest,
        TargetPolicy::Leader,
        TargetPolicy::Nearest,
        TargetPolicy::Nearest,
    ]))?;
    assert_eq!(prepared.targets[..2], [ActorId(2), ActorId(2)]);
    assert_eq!(prepared.initial_actors()[0].facing_direction, [1., 0., 0.]);
    assert_eq!(prepared.initial_actors()[1].facing_direction, [-1., 0., 0.]);
    Ok(())
}

#[test]
fn entry_spread_uses_distinct_opponents_and_replays_deterministically() -> Result<()> {
    let prepare = || {
        let actors: Vec<_> = [0., 10., 20., 100., 200., 300.]
            .into_iter()
            .enumerate()
            .map(|(index, x)| actor(if index < 3 { Side::Party } else { Side::Enemy }, x))
            .collect();
        let mut prepared = PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            Default::default(),
            42,
        )?;
        prepared.targets = [4, 4, 4, 0, 0, 0].map(ActorId).to_vec();
        prepared.with_entry_choices(choices(&[
            TargetPolicy::Nearest,
            TargetPolicy::Spread,
            TargetPolicy::Spread,
            TargetPolicy::Nearest,
            TargetPolicy::Nearest,
            TargetPolicy::Nearest,
        ]))
    };
    let first = prepare()?;
    let second = prepare()?;
    let targets = &first.targets[..3];
    assert!(
        targets
            .iter()
            .all(|target| (3..6).contains(&target.index()))
    );
    assert_eq!(
        targets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(first.targets, second.targets);
    assert_eq!(first.random_seed, second.random_seed);
    Ok(())
}

#[test]
fn entry_rejects_unavailable_targets_and_party_self_targeting() -> Result<()> {
    let prepare = |party, enemy, available: bool| {
        let mut opponent = actor(Side::Enemy, 100.);
        if !available {
            opponent.availability = crate::ActorAvailability::Dead;
        }
        PreparedBattle::new(
            vec![
                (actor(Side::Party, 0.), Default::default()),
                (opponent, Default::default()),
            ],
            Default::default(),
            42,
        )?
        .with_entry_choices(choices(&[party, enemy]))
    };
    let prepared = prepare(TargetPolicy::Nearest, TargetPolicy::SelfTarget, true)?;
    assert_eq!(prepared.targets, [ActorId(1), ActorId(1)]);
    assert!(prepare(TargetPolicy::SelfTarget, TargetPolicy::Nearest, true).is_err());
    assert!(prepare(TargetPolicy::Nearest, TargetPolicy::Nearest, false).is_err());
    Ok(())
}

#[test]
fn activation_preserves_explicit_targets_for_controllers_and_decisions() -> Result<()> {
    let targets = [4, 3, 4, 0, 1];
    let mut prepared = crate::companion::tests::prepared()?;
    prepared.targets = targets.map(ActorId).to_vec();
    let battle = prepared.finish()?;
    for (index, target) in targets.into_iter().enumerate() {
        assert_eq!(battle.target(ActorId(index as u8)), Some(ActorId(target)));
        if let Some(control) = &battle.runtime[index].control {
            assert_eq!(control.attack_target, ActorId(target));
        }
    }
    let mut invalid = crate::companion::tests::prepared()?;
    invalid.targets = [1, 3, 4, 0, 1].map(ActorId).to_vec();
    assert!(invalid.finish().is_err());
    Ok(())
}
