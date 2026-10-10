//! Petrification persists as an ailment; presentation chooses a pose on entry.
use super::item_tests::{escape, native_fixture};
use super::*;

#[test]
fn petrification_and_vitals_commit_without_actor_models() -> Result<()> {
    let mut fixture = native_fixture(
        &[1, 3],
        |party, _, _| {
            party.members[2].ailments.petrified = true;
            Ok(())
        },
        |mut actors, _| {
            actors[0].hp -= 1;
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
    fixture.candidate.sync_party(&fixture.battle)?;
    let outcome = escape(&mut fixture.candidate, &mut fixture.battle)?;
    let completed = fixture.candidate.finish(&fixture.battle, &outcome)?;
    let saved: Party = serde_json::from_slice(&serde_json::to_vec(&completed.party)?)?;
    assert!(saved.members[2].ailments.petrified);
    assert_eq!(saved.members[0].hp, fixture.field.members[0].hp - 1);
    let next = native_fixture(
        &[1, 3],
        |party, _, _| {
            *party = saved;
            Ok(())
        },
        |actors, _| {
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
    assert_eq!(
        next.battle.actors()[1].availability,
        ActorAvailability::Petrified
    );
    Ok(())
}
