use super::*;
use resonance_battle::{ActionRequest, Affinity, BattleInput, Control, Cue, Element, Side};

#[test]
#[ignore = "requires cooked battle and save assets; CPU only"]
fn cooked_elemental_contact_knowledge_survives_save_and_next_encounter() -> Result<()> {
    let mut monster_for_scan = None;
    for (affinity, weak, resistant) in [
        (Affinity::Weak, true, false),
        (Affinity::Immune, false, true),
    ] {
        let LoadedFixture {
            inputs,
            modification: _modification,
            party,
            session,
            menus,
            catalogue,
        } = load_fixture(
            &[1, 3],
            0,
            &[(3, 66)],
            Diagnostics::default(),
            |_, _, _, _, _| Ok(()),
        )?;
        let prepared = inputs.prepare_fixture(
            &menus,
            PrepareOptions {
                hunting_symbol: None,
                event_flags: Default::default(),
                random_seed: 0x2345,
                map: 332,
                world_music: 0,
                story: 2500,
                colette_state: 0,
                devils_arms_unlocked: false,
                victory_story_flags: [true; 2],
                overlimit_boost: false,
            },
            |sound| Ok(Some(sound)),
            |participants| {
                for (index, (actor, setup)) in participants.iter_mut().enumerate() {
                    actor.control = Control::Manual;
                    actor.position = [[-300., 0., -100.], [-200., 0., 0.], [200., 0., 0.]][index];
                    if actor.side == Side::Enemy {
                        actor.equipment.max_hp = 30000;
                        actor.hp = 30000;
                        actor.equipment.affinities[Element::Fire as usize + 1] = affinity;
                        setup.enemy_decision = None;
                        setup.recovery_return = None;
                    }
                }
                Ok(())
            },
        )?;
        let monster = prepared.results.enemies[0].reward.monster;
        monster_for_scan = Some(monster);
        let target = prepared.results.enemies[0].actor;
        let mut battle = prepared.core;
        let mut candidate = Candidate::new(
            prepared.results,
            battle.actors(),
            party,
            Default::default(),
            session,
            menus,
            catalogue,
        )?;
        enter_battle(&mut candidate, &mut battle)?;
        assert!(!battle.actors()[target.index()].weakness_known);
        assert!(!battle.actors()[target.index()].resistance_known);
        let caster = ActorId::from_index(1)?;
        let action = battle
            .prepared_techniques(caster)
            .iter()
            .find(|row| row.catalogue.0 == 66)
            .context("missing Fire Ball")?
            .action;
        battle.step(BattleInput {
            actions: vec![ActionRequest {
                actor: caster,
                action,
                target,
            }],
            ..Default::default()
        })?;
        let mut contact = false;
        for _ in 0..1600 {
            let frame = step(&mut candidate, &mut battle)?;
            if frame.cues.iter().any(|cue| matches!(cue, Cue::Hit { actor, owner, .. } if *actor == target && *owner == caster)) {
                contact = true;
                break;
            }
        }
        ensure!(contact, "spell never reached the enemy");
        assert_eq!(battle.actors()[target.index()].weakness_known, weak);
        assert_eq!(battle.actors()[target.index()].resistance_known, resistant);
        let outcome = escape(&mut candidate, &mut battle)?;
        let completed = candidate.finish(&battle, &outcome)?;
        let restored: Party = serde_json::from_slice(&serde_json::to_vec(&completed.party)?)?;
        assert_eq!(restored.monsters[&monster].weakness_known, weak);
        assert_eq!(restored.monsters[&monster].resistance_known, resistant);
        assert_eq!(
            restored.monsters[&monster].script_flags(),
            if weak { 0x41 } else { 0x81 }
        );
        let next = prepared_fixture_with_party(&[1, 3], 0, &[], |party, _, _| {
            party.monsters = restored.monsters.clone();
            Ok(())
        })?;
        let enemy = next
            .battle
            .actors()
            .iter()
            .find(|actor| actor.side == Side::Enemy)
            .unwrap();
        assert_eq!(enemy.weakness_known, weak);
        assert_eq!(enemy.resistance_known, resistant);
    }
    let monster = monster_for_scan.unwrap();
    let scanned = prepared_fixture_with_party(&[1, 3], 0, &[], |party, _, _| {
        party.monsters.entry(monster).or_default().scanned = true;
        Ok(())
    })?;
    let enemy = scanned
        .battle
        .actors()
        .iter()
        .find(|actor| actor.side == Side::Enemy)
        .unwrap();
    assert!(enemy.weakness_known && enemy.resistance_known);
    let snapshot = scanned.candidate.snapshot_party(&scanned.battle)?;
    assert!(!snapshot.monsters[&monster].weakness_known);
    assert!(!snapshot.monsters[&monster].resistance_known);
    Ok(())
}
