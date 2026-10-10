use crate::Side;
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy)]
pub struct TargetActor {
    pub side: Side,
    pub position: [f32; 3],
    pub hp: i32,
    pub hp_percent: i16,
    pub flying: bool,
    pub casting: bool,
    pub available: bool,
    pub hidden: bool,
    pub dying: bool,
    pub target: Option<usize>,
}

pub use resonance_content::battle_action::TargetPolicy;

pub fn select_target(
    actors: &[TargetActor],
    owner: usize,
    policy: TargetPolicy,
    random: &mut impl FnMut() -> u16,
) -> Result<Option<usize>> {
    ensure!(owner < actors.len(), "invalid target owner");
    ensure!(
        actors.iter().all(
            |actor| actor.target.is_none_or(|target| target < actors.len())
                && actor.position.iter().all(|value| value.is_finite())
        ),
        "invalid targeting roster"
    );
    let side = actors[owner].side;
    let eligible = |actor: &TargetActor| actor.available && !actor.hidden && !actor.dying;
    if policy == TargetPolicy::SelfTarget {
        return Ok(eligible(&actors[owner]).then_some(owner));
    }
    let candidates: Vec<_> = actors
        .iter()
        .enumerate()
        .filter(|(_, actor)| actor.side != side && eligible(actor))
        .map(|(index, _)| index)
        .collect();
    let distance = |from: usize, to: usize| {
        let a = actors[from].position;
        let b = actors[to].position;
        (f64::from(a[0]) - f64::from(b[0])).hypot(f64::from(a[2]) - f64::from(b[2]))
    };
    let nearest = |from, candidates: &[usize]| {
        candidates
            .iter()
            .copied()
            .min_by(|&a, &b| distance(from, a).total_cmp(&distance(from, b)))
    };
    let mut spread = || {
        let unclaimed: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                !actors.iter().enumerate().any(|(index, actor)| {
                    index != owner
                        && actor.side == side
                        && eligible(actor)
                        && actor.target == Some(*candidate)
                })
            })
            .collect();
        if unclaimed.is_empty() {
            nearest(owner, &candidates)
        } else {
            Some(unclaimed[usize::from(random()) % unclaimed.len()])
        }
    };
    Ok(match policy {
        TargetPolicy::Nearest => nearest(owner, &candidates),
        TargetPolicy::Farthest => candidates
            .iter()
            .copied()
            .max_by(|&a, &b| distance(owner, a).total_cmp(&distance(owner, b))),
        TargetPolicy::LowestHp => candidates
            .iter()
            .copied()
            .min_by_key(|&index| actors[index].hp),
        TargetPolicy::Leader => actors
            .iter()
            .find(|actor| actor.side == side && eligible(actor))
            .and_then(|actor| actor.target)
            .filter(|target| candidates.contains(target))
            .or_else(|| nearest(owner, &candidates)),
        TargetPolicy::Protect => actors
            .iter()
            .enumerate()
            .filter(|(_, actor)| actor.side == side && eligible(actor))
            .min_by_key(|(_, actor)| actor.hp_percent)
            .and_then(|(ally, _)| nearest(ally, &candidates)),
        TargetPolicy::Flying | TargetPolicy::Casting => {
            let preferred: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|&index| {
                    if policy == TargetPolicy::Flying {
                        actors[index].flying
                    } else {
                        actors[index].casting
                    }
                })
                .collect();
            nearest(owner, &preferred).or_else(spread)
        }
        TargetPolicy::Spread => spread(),
        TargetPolicy::SelfTarget => unreachable!(),
    })
}

impl crate::Battle {
    pub(crate) fn retarget_after_defeat(&mut self, victim: crate::ActorId) -> Result<()> {
        let side = self.actors[victim.index()].side;
        if !self
            .actors
            .iter()
            .any(|actor| actor.side == side && actor.available())
        {
            return Ok(());
        }
        let actors: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| TargetActor {
                side: actor.side,
                position: actor.position,
                hp: actor.hp,
                hp_percent: actor.hp_percent(),
                flying: actor.movement.flying,
                casting: matches!(
                    self.activity(crate::ActorId(index as u8)),
                    crate::Activity::Casting { .. }
                ),
                available: actor.available(),
                hidden: false,
                dying: false,
                target: None,
            })
            .collect();
        for index in 0..self.actors.len() {
            let owner = crate::ActorId(index as u8);
            if self.actors[index].side != side && self.target(owner) == Some(victim) {
                let Some(target) =
                    select_target(&actors, index, TargetPolicy::Nearest, &mut || {
                        unreachable!("nearest target draws no RNG")
                    })?
                else {
                    continue;
                };
                self.set_decision_target(owner, crate::ActorId(target as u8))?;
            }
        }
        Ok(())
    }

    pub(crate) fn retarget_selected_opponent(&mut self) -> Result<()> {
        if self.phase() != crate::BattlePhase::Combat {
            return Ok(());
        }
        let Some(first) = self
            .actors
            .iter()
            .position(|actor| actor.side == Side::Party)
        else {
            return Ok(());
        };
        let mut controlled = self.actors.iter().enumerate().filter_map(|(index, actor)| {
            (actor.side == Side::Party && actor.control != crate::Control::Auto).then_some(index)
        });
        let leader = controlled.next().unwrap_or(first);
        if controlled.next().is_some() || !self.actors[leader].available() {
            return Ok(());
        }
        let leader = crate::ActorId(leader as u8);
        let Some(selected) = self.target(leader) else {
            return Ok(());
        };
        // An admitted action retains its target through recovery.
        if self.runtime[selected.index()].task().action().is_some()
            || self.approach_within_range(selected.index())
        {
            return Ok(());
        }
        let a = self.actors[leader.index()].position;
        let b = self.actors[selected.index()].position;
        if crate::distance::length([a[0] - b[0], 0., a[2] - b[2]]) < 350. {
            self.set_decision_target(selected, leader)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(side: Side, x: f32) -> TargetActor {
        TargetActor {
            side,
            position: [x, 0., 0.],
            hp: 100,
            hp_percent: 100,
            flying: false,
            casting: false,
            available: true,
            hidden: false,
            dying: false,
            target: None,
        }
    }

    fn select(actors: &[TargetActor], policy: TargetPolicy) -> Option<usize> {
        select_target(actors, 0, policy, &mut || 0).unwrap()
    }

    #[test]
    fn every_opponent_policy_requires_an_eligible_candidate() {
        let mut actors = [actor(Side::Party, 0.), actor(Side::Enemy, 1.)];
        for (available, hidden, dying) in [
            (false, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            actors[1].available = available;
            actors[1].hidden = hidden;
            actors[1].dying = dying;
            for policy in [
                TargetPolicy::Nearest,
                TargetPolicy::Farthest,
                TargetPolicy::Leader,
                TargetPolicy::Spread,
                TargetPolicy::Flying,
                TargetPolicy::Casting,
                TargetPolicy::LowestHp,
                TargetPolicy::Protect,
            ] {
                assert_eq!(select(&actors, policy), None, "{policy:?}");
            }
        }
        assert_eq!(select(&actors[..1], TargetPolicy::Nearest), None);
        assert_eq!(select(&actors, TargetPolicy::SelfTarget), Some(0));
        actors[0].available = false;
        assert_eq!(select(&actors, TargetPolicy::SelfTarget), None);
    }

    #[test]
    fn distance_and_health_selection_have_no_sentinel_limits() {
        let mut actors = [
            actor(Side::Party, 0.),
            actor(Side::Enemy, 30_000.),
            actor(Side::Enemy, 20_000.),
        ];
        actors[1].hp = 30_000_000;
        actors[2].hp = 20_000_000;
        actors[2].position[1] = 100_000.;
        assert_eq!(select(&actors, TargetPolicy::Nearest), Some(2));
        assert_eq!(select(&actors, TargetPolicy::Farthest), Some(1));
        assert_eq!(select(&actors, TargetPolicy::LowestHp), Some(2));
        assert_eq!(select(&actors, TargetPolicy::Protect), Some(2));
        actors[1].position[0] = f32::MAX;
        actors[2].position[0] = f32::MAX / 2.;
        actors[0].position[0] = -f32::MAX;
        assert_eq!(select(&actors, TargetPolicy::Nearest), Some(2));
    }

    #[test]
    fn spread_prefers_unclaimed_opponents_then_nearest_and_replays_deterministically() {
        let mut actors = [
            actor(Side::Party, 0.),
            actor(Side::Party, 1.),
            actor(Side::Enemy, 10.),
            actor(Side::Enemy, 20.),
        ];
        actors[1].target = Some(2);
        assert_eq!(select(&actors, TargetPolicy::Spread), Some(3));
        actors[1].available = false;
        let replay = || {
            let mut random = crate::Random::new(42);
            (0..20)
                .map(|_| {
                    select_target(&actors, 0, TargetPolicy::Spread, &mut || random.next_u16())
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(replay(), replay());
        actors[1].available = true;
        actors[3].hidden = true;
        assert_eq!(select(&actors, TargetPolicy::Spread), Some(2));
    }

    #[test]
    fn preferences_consider_live_opponent_and_ally_state() {
        let mut actors = [
            actor(Side::Party, 0.),
            actor(Side::Party, 200.),
            actor(Side::Enemy, 10.),
            actor(Side::Enemy, 220.),
        ];
        actors[0].target = Some(3);
        actors[1].hp_percent = 25;
        actors[3].flying = true;
        actors[3].casting = true;
        for policy in [
            TargetPolicy::Leader,
            TargetPolicy::Flying,
            TargetPolicy::Casting,
            TargetPolicy::Protect,
        ] {
            assert_eq!(select(&actors, policy), Some(3), "{policy:?}");
        }
        actors[3].hidden = true;
        for policy in [
            TargetPolicy::Leader,
            TargetPolicy::Flying,
            TargetPolicy::Casting,
            TargetPolicy::Protect,
        ] {
            assert_eq!(select(&actors, policy), Some(2), "{policy:?}");
        }
        actors[3].hidden = false;
        actors[1].available = false;
        assert_eq!(select(&actors, TargetPolicy::Protect), Some(2));
    }

    #[test]
    fn malformed_target_rosters_are_diagnosed() {
        let mut actors = [actor(Side::Party, 0.), actor(Side::Enemy, 10.)];
        actors[0].target = Some(2);
        assert!(select_target(&actors, 0, TargetPolicy::Nearest, &mut || 0).is_err());
        actors[0].target = None;
        actors[1].position[0] = f32::NAN;
        assert!(select_target(&actors, 0, TargetPolicy::Nearest, &mut || 0).is_err());
        assert!(TargetPolicy::try_from(0).is_err());
        assert!(TargetPolicy::try_from(10).is_err());
    }

    fn battle() -> crate::Battle {
        let mut actors = vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
            crate::tests::actor(Side::Enemy),
        ];
        actors[0].control = crate::Control::SemiAuto;
        actors[1].control = crate::Control::Auto;
        actors[2].position = [100., 0., 0.];
        actors[3].position = [200., 0., 0.];
        let mut action = crate::tests::action(3);
        crate::tests::attack_mut(&mut action).recovery = 3;
        let mut prepared = crate::PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            (vec![action]).into(),
            42,
        )
        .unwrap();
        crate::tests::assign_action(&mut prepared, 2, crate::ActionKey(0));
        let mut battle = prepared.finish().unwrap();
        battle
            .set_decision_target(crate::ActorId(0), crate::ActorId(2))
            .unwrap();
        battle
            .set_decision_target(crate::ActorId(1), crate::ActorId(2))
            .unwrap();
        battle
            .set_decision_target(crate::ActorId(2), crate::ActorId(1))
            .unwrap();
        battle
    }

    #[test]
    fn defeat_retargets_remaining_opponents() -> Result<()> {
        let mut battle = battle();
        battle.actors[2].availability = crate::ActorAvailability::Dead;

        battle.retarget_after_defeat(crate::ActorId(2))?;
        assert_eq!(battle.target(crate::ActorId(0)), Some(crate::ActorId(3)));
        assert_eq!(battle.target(crate::ActorId(1)), Some(crate::ActorId(3)));

        battle.actors[3].availability = crate::ActorAvailability::Dead;
        assert_eq!(
            battle.decision_target(crate::ActorId(0), TargetPolicy::Nearest)?,
            None
        );
        Ok(())
    }

    #[test]
    fn selected_opponent_keeps_committed_actions_and_respects_pause() -> Result<()> {
        let mut battle = battle();
        battle.start(
            crate::ActionRequest {
                actor: crate::ActorId(2),
                action: crate::ActionKey(0),
                target: crate::ActorId(1),
            },
            &mut vec![],
        )?;
        battle.retarget_selected_opponent()?;
        assert_eq!(battle.target(crate::ActorId(2)), Some(crate::ActorId(1)));
        battle.step(crate::BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(battle.target(crate::ActorId(2)), Some(crate::ActorId(1)));
        let mut recovered = false;
        for _ in 0..30 {
            let frame = battle.step(crate::BattleInput::default())?;
            let activity = frame.actors[2].activity;
            if activity == crate::Activity::Idle {
                break;
            }
            recovered |= activity == crate::Activity::Recovering;
            assert_eq!(battle.target(crate::ActorId(2)), Some(crate::ActorId(1)));
        }
        assert!(recovered);
        battle.step(crate::BattleInput::default())?;
        assert_eq!(battle.target(crate::ActorId(2)), Some(crate::ActorId(0)));
        Ok(())
    }
}
