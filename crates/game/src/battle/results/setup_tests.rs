use super::item_tests::{PreparedFixture, prepared_fixture_with_party};
use super::*;
use resonance_content::menu_data::ex_effect as ex;
use resonance_events::party::StatBuff;

#[test]
#[ignore = "requires prepared battle assets; CPU only"]
fn startup_normalizes_formation_identities_and_preserves_reserve_and_absent_members() -> Result<()>
{
    let fixture = prepared_fixture_with_party(&[9, 2, 1, 3, 4, 5], 2, &[], |party, _, _| {
        for member in &mut party.members {
            member.queued_buffs.insert(StatBuff::DefenseUp);
        }
        party.members[0].equipment[0] = 136;
        let genis = &mut party.members[2];
        genis.ex_skills[0] = ex::ADDITIONAL_COMBO;
        genis.ex_gems[0] = 3;
        genis.technique_balance = -99;
        Ok(())
    })?;
    assert!(fixture.candidate.party.battles.lloyd_non_wooden_blade_used);
    assert!(!fixture.field.battles.lloyd_non_wooden_blade_used);
    let genis = fixture
        .candidate
        .setup
        .actors
        .iter()
        .find(|&&(_, character)| character == 3)
        .unwrap()
        .0;
    assert_eq!(fixture.field.members[2].technique_balance, -99);
    assert_eq!(fixture.candidate.party.members[2].technique_balance, -100);
    assert_eq!(
        fixture.battle.actors()[genis.index()]
            .equipment
            .contact
            .technique_balance,
        -100
    );
    for (index, member) in fixture.candidate.party.members.iter().enumerate() {
        assert_eq!(
            member.queued_buffs,
            if [0, 1, 2, 8].contains(&index) {
                Default::default()
            } else {
                [StatBuff::DefenseUp].into()
            }
        );
    }
    assert!(
        fixture
            .field
            .members
            .iter()
            .all(|member| member.queued_buffs == [StatBuff::DefenseUp].into())
    );
    Ok(())
}

#[test]
#[ignore = "requires current victory publications; CPU only"]
fn missing_victory_motions_fall_back_to_posture_and_strict_preparation_rejects_them() -> Result<()>
{
    use resonance_content::diagnostics::Diagnostics;
    let prepare = |paranoid| -> Result<PreparedFixture> {
        let mut fixture = super::item_tests::load_fixture(
            &[1],
            2,
            &[],
            Diagnostics::new(paranoid),
            |_, _, _, _| Ok(()),
        )?;
        fixture.inputs.victory.groups.clear();
        for row in &fixture.inputs.victory.ordinary {
            if row.character == 1 {
                fixture.inputs.files.remove(&row.motion);
            }
        }
        fixture.prepare(2500, |sound| Ok(Some(sound)))
    };
    let mut fixture = prepare(false)?;
    assert!(fixture.battle.diagnostics().has_errors());
    assert!(fixture.candidate.setup.performances.is_empty());
    assert_eq!(fixture.candidate.setup.postures.len(), 1);
    fixture.candidate.select_victory(&fixture.battle, 0, 0)?;
    assert_eq!(fixture.candidate.selection().unwrap().pose, None);
    assert!(prepare(true).is_err());
    Ok(())
}

#[test]
#[ignore = "requires current battle, item and progression publications; CPU only"]
fn victory_commits_item_rewards_and_learned_techniques_once() -> Result<()> {
    use resonance_battle::{Control, Side, item::Release};
    const REGAL: usize = 7;
    const HEALER: u16 = 192;
    let mut fixture = prepared_fixture_with_party(&[8], 2, &[(8, 176)], |party, session, _| {
        party
            .raise_level(session, REGAL, 38, None, || 7)
            .map_err(anyhow::Error::msg)?;
        let member = &mut party.members[REGAL];
        member.techniques = [176].into();
        member.shortcuts = [176, 0, 0, 0];
        member.experience = session.experience[39] - 1;
        member.hp /= 2;
        party.settings.battle_controls = [0; 4];
        Ok(())
    })?;
    let actor = fixture.candidate.setup.actors[0].0;
    assert_eq!(
        fixture.battle.technique_is_current(actor, HEALER),
        Some(false)
    );
    let before_items = fixture.candidate.party.items[&1];
    let before_battles = fixture.field.battles.total;
    let mut item_started = false;
    let mut item_used = false;
    let mut result_updates = 0;
    let mut awards = None;
    let mut outcome = None;
    for _ in 0..4000 {
        if !item_started && fixture.battle.phase() != BattlePhase::Entry {
            fixture.battle.queue_item(Release {
                user: actor,
                target: actor,
                item: 1,
            })?;
            item_started = true;
        }
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            crate::battle::lifecycle::Input {
                confirm: result_updates >= 30,
                ..Default::default()
            },
            &mut fixture.candidate,
            &mut super::item_tests::Display,
        )?;
        if !item_used && fixture.battle.ledger().items[actor.index()] == 1 {
            item_used = true;
            assert_eq!(fixture.candidate.party.items[&1], before_items - 1);
            fixture.battle.set_control_mode(actor, Control::Auto)?;
            for (index, enemy) in fixture.battle.actors().to_vec().iter().enumerate() {
                if enemy.side == Side::Enemy {
                    fixture.battle.set_actor_vitals(
                        ActorId::from_index(index)?,
                        1,
                        enemy.equipment.max_hp,
                        enemy.tp,
                        enemy.equipment.max_tp,
                    )?;
                }
            }
        }
        if let Some(pending) = fixture.candidate.pending_results() {
            result_updates += 1;
            let regal = &pending.party.members[REGAL];
            let current = (pending.party.gald, regal.experience);
            assert_eq!(*awards.get_or_insert(current), current);
            assert_eq!(pending.party.battles.total, before_battles + 1);
            assert_eq!(regal.level, 39);
            assert!(regal.techniques.contains(&HEALER));
            assert!(regal.shortcuts.contains(&HEALER));
            assert_eq!(
                fixture.battle.technique_is_current(actor, HEALER),
                Some(false)
            );
        }
        if frame.outcome.is_some() {
            outcome = frame.outcome;
            break;
        }
    }
    let outcome = outcome.context("victory did not finish")?;
    assert!(item_used && result_updates >= 30);
    assert_eq!(fixture.battle.phase(), BattlePhase::Finished);
    assert!(fixture.candidate.pending_results().unwrap().accepted);
    let completed = fixture.candidate.finish(&fixture.battle, &outcome)?;
    assert_eq!(completed.result, BattleResult::Victory);
    assert_eq!(
        (
            completed.party.gald,
            completed.party.members[REGAL].experience
        ),
        awards.unwrap()
    );
    assert_eq!(completed.party.battles.total, before_battles + 1);
    assert_eq!(completed.party.items[&1], before_items - 1);
    let techniques = completed.party.members[REGAL].techniques.clone();
    let shortcuts = completed.party.members[REGAL].shortcuts;
    let mut field = super::strategy_tests::field_call(fixture.field)?;
    let request = field
        .world
        .battle_request
        .take()
        .context("missing field caller")?;
    completed.commit(&mut field.world, &request)?;
    let saved: Party =
        serde_json::from_slice(&serde_json::to_vec(field.world.party.as_ref().unwrap())?)?;
    assert_eq!(saved.items[&1], before_items - 1);
    assert_eq!(saved.members[REGAL].techniques, techniques);
    assert_eq!(saved.members[REGAL].shortcuts, shortcuts);
    let next = prepared_fixture_with_party(&[8], 2, &[], |party, _, _| {
        *party = saved;
        Ok(())
    })?;
    let actor = next.candidate.setup.actors[0].0;
    assert_eq!(next.battle.current_techniques(actor), Some(&techniques));
    assert_eq!(next.battle.shortcuts(actor), Some(&shortcuts));
    assert_eq!(next.battle.technique_uses(actor, HEALER), Some(0));
    Ok(())
}
