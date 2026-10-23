//! Menu-only updates preserve the remaining lifetime of live conditions.
use super::item_tests::{prepared_fixture_with_party, refresh_equipment, release, step};
use super::*;
use resonance_battle::conditions::Condition::{AttackUp, Enchanted, Paralysis};

#[test]
#[ignore = "requires cooked party/profile/equipment assets; CPU only"]
fn strategy_and_equipment_preserve_live_conditions_and_enchantment() -> Result<()> {
    const WATER_ENCHANTMENT_ITEM: u16 = 44;
    let mut f = prepared_fixture_with_party(&[4, 1], 2, &[], |party, session, _| {
        party.members[3].ailments.paralysis = true;
        party.members[3]
            .queued_buffs
            .insert(resonance_events::party::StatBuff::AttackUp);
        party
            .change_item(session, WATER_ENCHANTMENT_ITEM, 1)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    })?;
    let actor = f.candidate.setup.actors[0].0;
    let initial = f.battle.actors()[actor.index()].conditions.clone();
    for _ in 0..600 {
        step(&mut f.candidate, &mut f.battle)?;
        if f.battle.phase() != resonance_battle::BattlePhase::Entry
            && f.battle.actors()[actor.index()]
                .conditions
                .remaining(Paralysis)
                < initial.remaining(Paralysis)
        {
            break;
        }
    }
    // A live water enchantment must survive the same menu round trip as saved ailments.
    release(
        &mut f.candidate,
        &mut f.battle,
        resonance_battle::item::Release {
            user: actor,
            target: actor,
            item: WATER_ENCHANTMENT_ITEM,
        },
    )?;
    let before = f.battle.actors()[actor.index()].conditions.clone();
    for condition in [Paralysis, AttackUp] {
        assert!(before.remaining(condition).unwrap() > 0);
        assert!(before.remaining(condition) < initial.remaining(condition));
    }
    assert!(before.base().contains(Enchanted));
    let elements = f.battle.actors()[actor.index()].elements;
    assert_eq!(elements.enchantment, Some(resonance_battle::Element::Water));
    f.candidate.begin_strategy(&f.battle)?;
    f.candidate.finish_strategy(&mut f.battle)?;
    assert_eq!(f.battle.actors()[actor.index()].conditions, before);
    assert_eq!(f.battle.actors()[actor.index()].elements, elements);

    refresh_equipment(&mut f.candidate, &mut f.battle)?;
    assert_eq!(f.battle.actors()[actor.index()].conditions, before);
    assert_eq!(f.battle.actors()[actor.index()].elements, elements);
    step(&mut f.candidate, &mut f.battle)?;
    for condition in [Paralysis, AttackUp] {
        assert_eq!(
            f.battle.actors()[actor.index()]
                .conditions
                .remaining(condition),
            before.remaining(condition).map(|ticks| ticks - 1)
        );
    }
    Ok(())
}
