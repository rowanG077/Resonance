use super::*;
use resonance_content::prepared::Files;

#[test]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn supported_actions_prepare_independently_of_saved_membership() -> Result<()> {
    use resonance_content::{diagnostics::Diagnostics, prepared::Cache, session::SessionData};
    use resonance_events::{
        battle::{DefeatPolicy, Setup},
        party::Party,
    };
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
        });
    let mut cache = Cache::default();
    for paranoid in [false, true] {
        let diagnostics = Diagnostics::new(paranoid);
        let files = Files::load_with_diagnostics(
            &root,
            &["fields/map-332.preload.json"],
            &mut cache,
            || false,
            diagnostics.clone(),
        )?;
        let menus: MenuData = files.json("game/menu-data.json")?;
        let mut session: SessionData = files.json("game/session-data.json")?;
        session.rules = Some(Arc::new(menus.clone()));
        let mut party = Party::new(&session, Default::default())?;
        party.formation = vec![1];
        party.members[0].techniques = [1, 2].into();
        party.members[0].shortcuts = [1, 2, 0, 0];
        party.members[0].technique_uses = [(1, 17), (2, 23)].into();
        party.validate(&session)?;
        let inputs = Inputs::load(
            &root,
            &files,
            &menus,
            &session,
            &party,
            Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(2),
                arena: 13,
                music: None,
                defeat: DefeatPolicy::ResumeEvent,
            },
            &mut cache,
            || false,
        )?;
        let result = inputs.prepare(
            &menus,
            PrepareOptions {
                random_seed: 1234,
                map: 332,
                world_music: 0,
                story: 2500,
                colette_state: 0,
                story3: false,
                victory_story_flags: [false; 2],
                devils_arms_unlocked: false,
                overlimit_boost: false,
            },
            |request| Ok(Some(request)),
        );
        assert!(
            diagnostics
                .entries()
                .iter()
                .any(|entry| entry.scope == "battle technique preparation"
                    && entry.message.contains("technique 2 for character 1"))
        );
        if paranoid {
            assert!(
                result
                    .err()
                    .context("paranoid preparation accepted an unsupported action")?
                    .to_string()
                    .contains("technique 2 for character 1")
            );
            continue;
        }
        let mut battle = result?.core;
        let actor = battle.actor_ids().next().unwrap();
        let supported = battle
            .prepared_technique(actor, 1)
            .context("supported Demon Fang missing")?
            .action;
        assert!(battle.prepared_technique(actor, 2).is_none());
        assert_eq!(battle.technique_is_current(actor, 2), Some(true));
        assert_eq!(battle.technique_uses(actor, 1), Some(17));
        assert_eq!(battle.technique_uses(actor, 2), Some(23));
        assert_eq!(battle.shortcuts(actor).unwrap(), &[1, 2, 0, 0]);
        let guard = battle
            .prepared_technique(actor, 34)
            .context("dormant Special Guard capacity missing")?
            .action;
        assert_eq!(battle.technique_is_current(actor, 34), Some(false));
        for _ in 0..600 {
            if battle.phase() != resonance_battle::BattlePhase::Entry {
                break;
            }
            battle.step(Default::default())?;
        }
        assert!(battle.phase() != resonance_battle::BattlePhase::Entry);
        assert_eq!(battle.record_technique_acquisition(actor, 34)?, guard);
        assert_eq!(battle.shortcuts(actor).unwrap(), &[1, 2, 34, 0]);
        battle.forget_technique(actor, guard)?;
        battle.prepare_shortcut(actor, 1, None)?.commit();
        battle.record_technique_acquisition(actor, 34)?;
        assert_eq!(battle.shortcuts(actor).unwrap(), &[1, 34, 0, 0]);
        battle.prepare_shortcut(actor, 1, Some(supported))?.commit();
        battle.forget_technique(actor, guard)?;
        battle.record_technique_acquisition(actor, 34)?;
        assert_eq!(battle.shortcuts(actor).unwrap(), &[1, 1, 34, 0]);
        assert_eq!(battle.technique_is_current(actor, 2), Some(true));
        let target = battle
            .target(actor)
            .context("prepared action has no target")?;
        battle.step(resonance_battle::BattleInput {
            actions: vec![resonance_battle::ActionRequest {
                actor,
                action: supported,
                target,
            }],
            ..Default::default()
        })?;
        assert_eq!(battle.technique_uses(actor, 1), Some(18));
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn optional_body_art_preserves_placement_casting_items_and_results() -> Result<()> {
    use crate::battle::results::item_tests::{escape, load_fixture, release, step};
    use resonance_battle::{
        ActionRequest, Activity, BattleInput, ControlInput, Cue, item::Release,
    };
    use resonance_content::diagnostics::Diagnostics;
    let mut placement = None;
    for fault in [
        "intact",
        "entry clips",
        "body",
        "enemy attachment",
        "enemy descriptor",
        "enemy definition",
    ] {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let loaded = load_fixture(
                &[3],
                2,
                &[(3, 66)],
                diagnostics.clone(),
                |party, _, _, files| {
                    party.settings.battle_controls[0] = 1;

                    if fault == "enemy descriptor" {
                        files.remove(&resonance_content::battle_model::enemy_path(
                            battle::enemy::GHOST,
                        ));
                    }
                    if fault == "enemy definition" {
                        files.remove(&resonance_content::battle_enemy::path(battle::enemy::GHOST));
                    }
                    Ok(())
                },
            );
            if fault == "enemy definition" || (paranoid && fault == "enemy descriptor") {
                assert!(loaded.is_err(), "missing enemy input accepted: {fault}");
                continue;
            }
            let mut fixture = loaded?;
            let inputs = &mut fixture.inputs;
            if fault == "enemy descriptor" {
                assert!(!inputs.enemy_models.contains_key(&battle::enemy::GHOST));
                assert!(diagnostics.has_errors());
            }
            inputs.stage.actor_color = [40, 50, 60, 255];
            for spawn in &mut inputs.enemies.spawns {
                spawn.position = None;
            }
            for source in inputs.enemy_definitions.values_mut() {
                source.entry_row = resonance_content::battle_enemy::EntryRow::Random;
            }
            match fault {
                "entry clips" => {
                    for actor in &mut inputs.party_actors {
                        actor.motions.clear();
                    }
                    for (_, motions) in inputs.enemy_models.values_mut() {
                        motions.clear();
                    }
                }
                "body" => {
                    for actor in &mut inputs.party_actors {
                        actor.source.as_mut().unwrap().body.skeleton.bones[0].parent = Some(0);
                    }
                    for (source, _) in inputs.enemy_models.values_mut() {
                        source.body.skeleton.bones[0].parent = Some(0);
                    }
                }
                "enemy attachment" => {
                    inputs
                        .enemy_models
                        .get_mut(&49)
                        .unwrap()
                        .0
                        .body
                        .attachments
                        .clear();
                }
                "intact" | "enemy descriptor" => {}
                _ => unreachable!(),
            }
            let prepared = fixture.prepare(2500, |request| Ok(Some(request)));
            if paranoid && fault != "intact" {
                assert!(prepared.is_err(), "strict preparation accepted {fault}");
                assert!(diagnostics.has_errors());
                continue;
            }
            let mut fixture = prepared?;
            let battle = &mut fixture.battle;
            let candidate = &mut fixture.candidate;
            let before = (
                battle
                    .actors()
                    .iter()
                    .map(|actor| actor.position)
                    .collect::<Vec<_>>(),
                battle.random_state(),
            );
            if let Some(expected) = &placement {
                assert_eq!(&before, expected, "{fault}");
            } else {
                placement = Some(before);
            }

            for _ in 0..600 {
                if battle.phase() != resonance_battle::BattlePhase::Entry {
                    break;
                }
                step(candidate, battle)?;
            }
            assert!(
                battle.phase() != resonance_battle::BattlePhase::Entry,
                "{fault}"
            );
            let user = battle.actor_ids().next().unwrap();
            let origin = battle.actors()[user.index()].position;
            for _ in 0..4 {
                candidate.world_update(
                    battle,
                    BattleInput {
                        controllers: vec![ControlInput {
                            stick: [127, 0],
                            ..ControlInput::neutral(user)
                        }],
                        ..Default::default()
                    },
                )?;
            }
            assert_ne!(battle.actors()[user.index()].position, origin, "{fault}");
            release(
                candidate,
                battle,
                Release {
                    user,
                    target: user,
                    item: 1,
                },
            )?;
            assert_eq!(battle.ledger().items[user.index()], 1);
            assert_eq!(candidate.items().counts[&1], 1);
            for _ in 0..120 {
                if battle.activity(user) == Activity::Idle {
                    break;
                }
                step(candidate, battle)?;
            }
            let tp = battle.actors()[user.index()].tp;
            let action = battle.prepared_technique(user, 66).unwrap().action;
            let request = ActionRequest {
                actor: user,
                target: battle.target(user).context("missing opponent")?,
                action,
            };
            let mut released = false;
            for tick in 0..600 {
                let cues = candidate.world_update(
                    battle,
                    BattleInput {
                        actions: if tick == 0 { vec![request] } else { vec![] },
                        ..Default::default()
                    },
                )?;
                released |= cues.iter().any(|cue| matches!(cue, Cue::Released { .. }));
                if released && battle.activity(user) == Activity::Idle {
                    break;
                }
            }
            assert!(released, "{fault}");
            assert!(battle.actors()[user.index()].tp < tp, "{fault}");
            assert_eq!(battle.technique_uses(user, 66), Some(1));
            assert!(!battle.is_diagnostic(), "{fault}");
            let outcome = escape(candidate, battle)?;
            let completed = fixture.candidate.finish(battle, &outcome)?;
            assert_eq!(completed.party.items[&1], 1);
            assert_eq!(completed.party.members[2].technique_uses[&66], 1);
            assert_eq!(fixture.field.items[&1], 2);
        }
    }
    Ok(())
}
