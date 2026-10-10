//! Over Limit persistence through result rewards and battle entry.
use super::item_tests::PreparedFixture;
use super::*;
use resonance_battle::OverLimit;

fn set_gauges(candidate: &Candidate, battle: &mut Battle, gauges: [OverLimit; 4]) -> Result<()> {
    for (&(id, _), gauge) in candidate.setup.actors.iter().zip(gauges) {
        let actor = battle.actors()[id.index()].clone();
        battle.refresh_result_member(
            id,
            actor.hp,
            actor.equipment.max_hp,
            actor.tp,
            actor.equipment.max_tp,
            gauge,
            actor.is_petrified(),
            actor.conditions.layers(),
        )?;
    }
    Ok(())
}

#[test]
fn construct_rewards_keeps_partial_charge_and_clears_full_or_active_overlimit() -> Result<()> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        field,
        ..
    } = super::item_tests::native_fixture(
        &[4, 1, 2, 3],
        |_, _, _| Ok(()),
        |mut actors, _| {
            for actor in &mut actors {
                if actor.side == resonance_battle::Side::Enemy {
                    actor.hp = 0;
                    actor.availability = ActorAvailability::Dead;
                }
            }
            resonance_battle::PreparedBattle::new(
                (actors)
                    .into_iter()
                    .map(|actor| (actor, Default::default()))
                    .collect(),
                Default::default(),
                1,
            )?
            .finish()
        },
    )?;
    assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));
    battle.retire_combat()?;
    set_gauges(
        &candidate,
        &mut battle,
        [
            OverLimit::new(999)?,
            OverLimit::new(1000)?,
            OverLimit::active(1250)?,
            OverLimit::active(17)?,
        ],
    )?;
    let (actor, character) = candidate.setup.actors[0];
    candidate.selection = Some(Selection {
        actor,
        character,
        pose: Some(0),
        group: None,
    });
    candidate.construct_rewards(&mut battle)?;
    for (&(id, character), (live, saved)) in
        candidate
            .setup
            .actors
            .iter()
            .zip([(999, 99), (0, 0), (0, 0), (0, 0)])
    {
        assert_eq!(battle.actors()[id.index()].overlimit.charge(), live);
        assert!(!battle.actors()[id.index()].overlimit.is_active());
        assert_eq!(
            candidate.party.members[usize::from(character - 1)].overlimit,
            saved
        );
    }
    let before = serde_json::to_value(&candidate.party)?;
    let random = battle.random_state();
    assert!(candidate.construct_rewards(&mut battle).is_err());
    assert_eq!(battle.random_state(), random);
    assert_eq!(serde_json::to_value(&candidate.party)?, before);
    let restored: Party = serde_json::from_value(before)?;
    assert_eq!(restored.members[1].overlimit, 0);
    assert!(field.members.iter().all(|member| member.overlimit == 0));
    Ok(())
}
