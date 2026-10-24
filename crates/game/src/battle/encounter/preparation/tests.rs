use super::*;
use resonance_battle::TechniqueId;

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
        let files = resonance_content::preparation::capture(
            &resonance_content::library::Library::open(&root, &[])?,
            &[],
            &mut cache,
            || false,
            diagnostics.clone(),
        )?;
        let menus: MenuData = resonance_content::menu_data::MenuData::load(&files)?;
        let mut session: SessionData = files.json("game/session-data.json")?;
        session.rules = Some(Arc::new(menus.clone()));
        let mut party = Party::new(&session, Default::default())?;
        party.battle_rules.modifiers = 0x0150;
        party.battle_rules.attack_adjustment = -50;
        party.battle_rules.defense_adjustment = 50;
        party.battle_rules.intelligence_adjustment = -25;
        party.formation = vec![1, 3];
        party.members[2].techniques.clear();
        party.members[2].disabled_techniques.clear();
        party.members[2].shortcuts = [0; 4];
        party.members[0].techniques = [1, 2].into();
        party.members[0].shortcuts = [1, 2, 0, 0];
        party.members[0].technique_uses = [(1, 17), (2, 23)].into();
        party.validate(&session)?;
        let inputs = Inputs::capture(
            &mut (&files)
                .library()
                .context("named library is required")?
                .snapshot(),
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
            (&files).diagnostics().clone(),
            &(|| false),
        )?;
        assert!(inputs.spell_scenes.contains_key(&213));
        let result = inputs.prepare(
            &menus,
            PrepareOptions {
                hunting_symbol: None,
                event_flags: Default::default(),
                random_seed: 1234,
                map: 332,
                world_music: 0,
                story: 2500,
                colette_state: 0,
                victory_story_flags: [false; 2],
                devils_arms_unlocked: false,
                overlimit_boost: false,
            },
            |request| Ok(Some(request)),
        )?;
        assert!(!diagnostics.has_errors());
        let mut battle = result.core;
        for actor in battle.actors() {
            assert_eq!(
                actor.combat_adjustments,
                if actor.side == resonance_battle::Side::Party {
                    resonance_battle::CombatAdjustments {
                        attack: -50,
                        defense: 50,
                        intelligence: -25,
                        attack_quarters: 1,
                        defense_eighths: 1,
                        accuracy_quarters: 1,
                    }
                } else {
                    Default::default()
                }
            );
        }
        let actor = battle.actor_ids().next().unwrap();
        let supported = battle
            .prepared_technique(actor, TechniqueId(1))
            .context("supported Demon Fang missing")?
            .action;
        assert!(battle.prepared_technique(actor, TechniqueId(2)).is_some());
        assert_eq!(
            battle.technique_is_current(actor, TechniqueId(2)),
            Some(true)
        );
        assert_eq!(battle.technique_uses(actor, TechniqueId(1)), Some(17));
        assert_eq!(battle.technique_uses(actor, TechniqueId(2)), Some(23));
        assert_eq!(
            battle.shortcuts(actor).unwrap(),
            &[Some(TechniqueId(1)), Some(TechniqueId(2)), None, None]
        );
        let guard = battle
            .prepared_technique(actor, TechniqueId(34))
            .context("dormant Special Guard capacity missing")?
            .action;
        assert_eq!(
            battle.technique_is_current(actor, TechniqueId(34)),
            Some(false)
        );
        let genis = battle.actor_ids().nth(1).unwrap();
        let stalagmite = battle
            .prepared_technique(genis, TechniqueId(75))
            .context("unlearned Stalagmite capacity missing")?
            .action;
        assert_eq!(
            battle.technique_is_current(genis, TechniqueId(75)),
            Some(false)
        );
        for _ in 0..600 {
            let frame = battle.step(Default::default())?;
            if battle.phase() == resonance_battle::BattlePhase::Combat && !frame.clock.paused() {
                break;
            }
        }
        assert!(battle.phase() != resonance_battle::BattlePhase::Entry);
        assert_eq!(
            battle.record_technique_acquisition(genis, TechniqueId(75))?,
            stalagmite
        );
        assert_eq!(
            battle.technique_is_current(genis, TechniqueId(75)),
            Some(true)
        );
        assert_eq!(
            battle.record_technique_acquisition(actor, TechniqueId(34))?,
            guard
        );
        assert_eq!(
            battle.shortcuts(actor).unwrap(),
            &[
                Some(TechniqueId(1)),
                Some(TechniqueId(2)),
                Some(TechniqueId(34)),
                None
            ]
        );
        battle.forget_technique(actor, guard)?;
        battle
            .prepare_shortcut(actor, resonance_battle::ShortcutSlot::Up, None)?
            .commit();
        battle.record_technique_acquisition(actor, TechniqueId(34))?;
        assert_eq!(
            battle.shortcuts(actor).unwrap(),
            &[Some(TechniqueId(1)), Some(TechniqueId(34)), None, None]
        );
        battle
            .prepare_shortcut(actor, resonance_battle::ShortcutSlot::Up, Some(supported))?
            .commit();
        battle.forget_technique(actor, guard)?;
        battle.record_technique_acquisition(actor, TechniqueId(34))?;
        assert_eq!(
            battle.shortcuts(actor).unwrap(),
            &[
                Some(TechniqueId(1)),
                Some(TechniqueId(1)),
                Some(TechniqueId(34)),
                None
            ]
        );
        assert_eq!(
            battle.technique_is_current(actor, TechniqueId(2)),
            Some(true)
        );
        let target = battle
            .target(actor)
            .context("prepared action has no target")?;
        battle.step(resonance_battle::BattleInput::from(
            resonance_battle::ActionRequest {
                actor,
                action: supported,
                target,
            },
        ))?;
        assert_eq!(battle.technique_uses(actor, TechniqueId(1)), Some(18));
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn optional_body_art_preserves_placement_casting_items_and_results() -> Result<()> {
    use crate::battle::results::item_tests::{enter_battle, escape, load_fixture, release, step};
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
                |party, _, _, snapshot, modification| {
                    party.settings.battle_controls[0] = 1;
                    if matches!(fault, "enemy descriptor" | "enemy definition") {
                        use resonance_content::battle_enemy::document;
                        let path = snapshot.resolve(
                            document::MAPPINGS,
                            u32::from(battle::enemy::GHOST),
                            Some("enemy.json"),
                        )?;
                        let mut entry: document::Entry =
                            snapshot.json(&path, resonance_content::preparation::JSON_LIMIT)?;
                        if fault == "enemy descriptor" {
                            entry.artwork = "battle/enemies/ghost/missing-model.json".into();
                        } else {
                            entry.rules = "battle/enemies/ghost/missing-rules.json".into();
                        }
                        let target = modification.join(path);
                        std::fs::create_dir_all(target.parent().context("missing enemy owner")?)?;
                        std::fs::write(target, serde_json::to_vec(&entry)?)?;
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
                        actor.source.body.skeleton.bones[0].parent = Some(0);
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
            if matches!(fault, "entry clips" | "body" | "enemy descriptor")
                || (paranoid && fault != "intact")
            {
                assert!(prepared.is_err(), "strict preparation accepted {fault}");
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

            enter_battle(candidate, battle)?;
            let user = battle.actor_ids().next().unwrap();
            let origin = battle.actors()[user.index()].position;
            for _ in 0..4 {
                candidate.world_update(
                    battle,
                    BattleInput::from(ControlInput {
                        stick: [127, 0],
                        ..ControlInput::neutral(user)
                    }),
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
            let action = battle
                .prepared_technique(user, TechniqueId(66))
                .unwrap()
                .action;
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
            assert_eq!(battle.technique_uses(user, TechniqueId(66)), Some(1));
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
