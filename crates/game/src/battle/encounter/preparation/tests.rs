use super::*;
use resonance_battle::TechniqueId;

#[test]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn supported_actions_prepare_independently_of_saved_membership() -> Result<()> {
    use resonance_content::{diagnostics::Diagnostics, session::SessionData};
    use resonance_events::{
        battle::{DefeatPolicy, Setup},
        party::Party,
    };
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
        });

    for paranoid in [false, true] {
        let diagnostics = Diagnostics::new(paranoid);
        let files = resonance_content::preparation::capture(
            &resonance_content::library::Library::open(&root, &[])?,
            &[],
            || false,
            diagnostics.clone(),
        )?;
        let menus: MenuData = resonance_content::menu_data::MenuData::load(&files)?;
        let mut session: SessionData = resonance_content::session::SessionData::load(&files)?;
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
        let inputs = Assets::capture(
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
        let stun = result.stun_particle.as_ref().context("stun bird missing")?;
        let render = result
            .effects
            .iter()
            .find(|effect| effect.resource == stun.resource)
            .context("stun artwork bank missing")?;
        let (member, template) = render
            .source
            .named_particle(resonance_content::battle_effect::document::STUN_BIRD)?;
        assert_eq!(stun.member, u16::from(member));
        assert_eq!(&stun.data, template);
        assert!(render.members.contains(&stun.member));
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
                        let mut entry: serde_json::Value =
                            snapshot.json(&path, resonance_content::preparation::JSON_LIMIT)?;
                        if fault == "enemy descriptor" {
                            entry["artwork"] = serde_json::json!({"model":"battle/enemies/ghost/missing-model.json"});
                        } else {
                            entry["rules"] = serde_json::json!({});
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

#[cfg(feature = "test-support")]
struct GhostWolfFixture {
    inputs: Assets,
    menus: MenuData,
    diagnostics: resonance_content::diagnostics::Diagnostics,
    authored: resonance_content::battle_martial::AuthoredAction,
}

#[cfg(feature = "test-support")]
impl GhostWolfFixture {
    fn load(formation: &[u8], ability_plus: bool) -> Result<Self> {
        Self::load_encounter(formation, ability_plus, 1)
    }

    fn load_encounter(formation: &[u8], ability_plus: bool, encounter: u16) -> Result<Self> {
        use resonance_content::{diagnostics::Diagnostics, library::Library, session::SessionData};
        use resonance_events::{
            battle::{DefeatPolicy, Encounter, Setup},
            party::Party,
        };
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| repo.join("local/all-assets"));
        let imported = format!(
            "Ghost Wolf requires current imported assets at {}",
            root.display()
        );
        let authored = serde_json::from_slice(
            &std::fs::read(root.join("battle/artes/ghost-wolf/action.json"))
                .with_context(|| imported.clone())?,
        )?;
        let library = Library::open_paths(&root, &[])?;
        let diagnostics = Diagnostics::new(true);
        let files =
            resonance_content::preparation::capture(&library, &[], || false, diagnostics.clone())?;
        let mut session = SessionData::load(&files)?;
        let menus = MenuData::load(&files)?;
        if ability_plus {
            session.rules = Some(Arc::new(menus.clone()));
        }
        let mut party = Party::new(&session, Default::default())?;
        party.formation = formation.to_vec();
        party.settings.battle_controls = [1; 4];
        assert!(!party.members[0].techniques.contains(&260), "{imported}");
        party
            .raise_level(&session, 0, 5, None, || 0)
            .map_err(anyhow::Error::msg)?;
        assert!(!party.members[0].techniques.contains(&260), "{imported}");
        party
            .raise_level(&session, 0, 6, None, || 0)
            .map_err(anyhow::Error::msg)?;
        assert!(party.members[0].techniques.contains(&260), "{imported}");
        if ability_plus {
            // Already learned, so the opening cannot acquire Demon Fang.
            party.members[0].techniques.extend([1, 14, 254]);
            party.members[0].ex_gems = [4, 0, 0, 0];
            party.members[0].ex_skills = [15, 0, 0, 0];
            party.members[0].base_stats[1] = 100;
            party.members[0].tp = 100;
        }
        party.validate(&session)?;
        let inputs = Assets::capture(
            &mut library.snapshot(),
            &menus,
            &session,
            &party,
            Setup {
                route: [0; 5],
                encounter: Encounter::Formation(encounter),
                arena: 13,
                music: None,
                defeat: DefeatPolicy::ResumeEvent,
            },
            diagnostics.clone(),
            &|| false,
        )?;
        assert!(inputs.authored_effects.contains_key(&260));
        assert!(inputs.party_actors[0].motions.contains_key(&110));
        Ok(Self {
            inputs,
            menus,
            diagnostics,
            authored,
        })
    }

    fn battle(&self, random_seed: u64) -> Result<resonance_battle::Battle> {
        let mut battle = self
            .inputs
            .prepare(
                &self.menus,
                PrepareOptions {
                    hunting_symbol: None,
                    event_flags: Default::default(),
                    random_seed,
                    map: 332,
                    world_music: 0,
                    story: 2500,
                    colette_state: 0,
                    victory_story_flags: [false; 2],
                    devils_arms_unlocked: false,
                    overlimit_boost: false,
                },
                |sound| Ok(Some(sound)),
            )?
            .core;
        battle.suppress_automatic_decisions();
        for _ in 0..600 {
            let frame = battle.step(Default::default())?;
            if battle.phase() == resonance_battle::BattlePhase::Combat && !frame.clock.paused() {
                return Ok(battle);
            }
        }
        anyhow::bail!("Ghost Wolf fixture did not reach active combat")
    }

    fn place(
        &self,
        battle: &mut resonance_battle::Battle,
        owner_position: [f32; 3],
        target_position: [f32; 3],
    ) -> Result<(resonance_battle::ActorId, resonance_battle::ActorId)> {
        use resonance_battle::{ActorId, FixtureMotion};
        let owner = ActorId::from_index(0)?;
        let target = ActorId::from_index(self.inputs.party_actors.len())?;
        let sign = if target_position[0] >= owner_position[0] {
            1_f32
        } else {
            -1.
        };
        for (actor, position, direction) in [
            (owner, owner_position, sign),
            (target, target_position, -sign),
        ] {
            // Placement and all facing directions rotate together for mirrored cases.
            let mut motion =
                serde_json::to_value(FixtureMotion::observe(&battle.actors()[actor.index()]))?;
            motion["position"] = serde_json::json!(position);
            motion["heading"] = serde_json::json!(direction * 90.);
            for field in ["facing_direction", "movement_direction", "target_direction"] {
                motion[field] = serde_json::json!([direction, 0., 0.]);
            }
            battle.restore_fixture_motion(actor, serde_json::from_value(motion)?)?;
        }
        battle.restore_fixture_target(owner, target)?;
        Ok((owner, target))
    }
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn ghost_wolf_contact_and_projectile_sounds_are_captured_before_playback() -> Result<()> {
    use resonance_content::{battle_audio, battle_normal::Event};

    let fixture = GhostWolfFixture::load(&[1], false)?;
    let audio = battle_audio::document::Catalogue::capture(
        &mut fixture
            .inputs
            .files
            .library()
            .context("named library required")?
            .snapshot(),
        &|| false,
    )?;
    let options = || PrepareOptions {
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
    };
    // Reuse Demon Fang's valid projectile recipe without adding artwork dependencies.
    let mut projectile = fixture
        .inputs
        .party_profiles
        .martial_attack(1, Character::Lloyd as u8)?
        .events
        .iter()
        .find_map(|(_, event)| matches!(event, Event::Projectile { .. }).then(|| event.clone()))
        .context("Demon Fang projectile missing")?;
    let (mut inputs, _, baseline) =
        fixture
            .inputs
            .prepare_named(&fixture.menus, options(), &audio)?;
    let cues = audio
        .resolved
        .effects
        .keys()
        .filter_map(|&id| u16::try_from(id).ok())
        .filter(|id| *id > 0 && !baseline.sounds.contains(id))
        .take(2)
        .collect::<Vec<_>>();
    assert_eq!(cues.len(), 2, "need otherwise unused valid sound cues");
    let path = inputs
        .catalogue
        .definition(260)?
        .action
        .clone()
        .context("authored action missing")?;
    let mut authored = fixture.authored;
    for (_, event) in &mut authored.attack.events {
        if let Event::Contact(contact) = event {
            contact.hit.sound = Some(cues[0]);
        }
    }
    let Event::Projectile { hit: Some(hit), .. } = &mut projectile else {
        anyhow::bail!("Demon Fang projectile hit missing");
    };
    hit.sound = Some(cues[1]);
    authored.attack.events.push((16, projectile));
    inputs
        .files
        .insert(path.clone(), serde_json::to_vec(&authored)?.into());
    let (mut inputs, _, selected) = inputs.prepare_named(&fixture.menus, options(), &audio)?;
    for cue in cues {
        assert!(
            selected.sounds.contains(&cue),
            "hit sound {cue} was not captured"
        );
    }
    assert!(!fixture.diagnostics.has_errors());

    for (_, event) in &mut authored.attack.events {
        if let Event::Contact(contact) = event {
            contact.hit.sound = Some(u16::MAX);
        }
    }
    inputs
        .files
        .insert(path, serde_json::to_vec(&authored)?.into());
    let error = inputs
        .prepare_named(&fixture.menus, options(), &audio)
        .err()
        .context("an unavailable contact cue must fail during preparation")?;
    assert!(format!("{error:#}").contains("unprepared battle sound 65535"));
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn ghost_wolf_executes_from_assets_with_hits_misses_and_interrupts() -> Result<()> {
    use resonance_battle::{ActionRequest, BattleInput, Cue};
    let fixture = GhostWolfFixture::load(&[1, 3], false)?;
    let raybit = GhostWolfFixture::load_encounter(&[1, 3], false, 134)?;
    let authored = &fixture.authored;
    let (traversal_at, traversal_duration, traversal_limit) = authored
        .attack
        .events
        .iter()
        .find_map(|(at, event)| match event {
            resonance_content::battle_normal::Event::TraverseTarget {
                duration,
                max_distance,
                ..
            } => Some((u32::from(*at), u32::from(*duration), *max_distance)),
            _ => None,
        })
        .context("Ghost Wolf traversal missing")?;
    let first_contact_at = authored
        .attack
        .events
        .iter()
        .filter_map(|(at, event)| {
            matches!(event, resonance_content::battle_normal::Event::Contact(_))
                .then_some(u32::from(*at))
        })
        .min()
        .context("Ghost Wolf contacts missing")?;
    let (voice_at, expected_voice) = authored
        .attack
        .events
        .iter()
        .find_map(|(at, event)| match event {
            resonance_content::battle_normal::Event::Voice(Some(sound)) => {
                Some((u32::from(*at), *sound))
            }
            _ => None,
        })
        .context("Ghost Wolf voice missing")?;
    let early_interrupt_at = traversal_at.min(first_contact_at).min(voice_at) / 2;
    let maximum_updates =
        2 * (u32::from(authored.attack.duration) + u32::from(authored.attack.recovery)) + 60;
    let mut missed_close_passes = Vec::new();
    for (mode, initial_distance) in [
        ("hit", 40.),
        ("hit", 60.),
        ("hit", 80.),
        ("hit", 100.),
        ("hit", 120.),
        ("hit", 200.),
        ("hit", 225.),
        ("hit", 250.),
        ("flying", 40.),
        ("flying", 80.),
        ("flying", -40.),
        ("flying", -80.),
        ("far", 600.),
        ("far", -600.),
        ("outside", 251.),
        ("away", 200.),
        ("away", -200.),
        ("miss", 120.),
        ("interrupt", 120.),
        ("cross_interrupt", 120.),
    ] {
        let fixture = if mode == "flying" { &raybit } else { &fixture };
        let expecting_hits = matches!(mode, "hit" | "flying");
        let direction = if initial_distance >= 0. { 1. } else { -1. };
        let mut battle = fixture.battle(1234)?;
        if mode == "flying" {
            // Raybit's bobbing pose exposes both a close-entry miss and a
            // finishing-cut miss. Preserve its native hovering altitude.
            for _ in 0..10 {
                battle.step(Default::default())?;
            }
        }
        let target_height = if mode == "miss" {
            500.
        } else {
            battle.actors()[2].position[1]
        };
        let (owner, target) = fixture.place(
            &mut battle,
            [0., 0., 0.],
            [initial_distance, target_height, 0.],
        )?;
        if matches!(mode, "far" | "outside" | "away") {
            battle.set_control_mode(owner, resonance_battle::Control::Manual)?;
        }
        if mode == "away" {
            let mut motion = serde_json::to_value(resonance_battle::FixtureMotion::observe(
                &battle.actors()[owner.index()],
            ))?;
            motion["heading"] = serde_json::json!(-direction * 90.);
            motion["facing_direction"] = serde_json::json!([-direction, 0., 0.]);
            motion["movement_direction"] = serde_json::json!([-direction, 0., 0.]);
            battle.restore_fixture_motion(owner, serde_json::from_value(motion)?)?;
        }
        let action = battle
            .prepared_technique(owner, TechniqueId(260))
            .context("authored arte unavailable")?
            .action;
        let tp = battle.actors()[0].tp;
        let hp = battle.actors()[2].hp;
        battle.observe_fixture_contact_damage();
        let mut frames = vec![battle.step(BattleInput::from(ActionRequest {
            actor: owner,
            target,
            action,
        }))?];
        assert_eq!(battle.actors()[0].tp, tp - 12);
        assert_eq!(battle.technique_uses(owner, TechniqueId(260)), Some(1));
        let mut interrupted = false;
        let mut pass_end_separation = None;
        for _ in 0..maximum_updates {
            let interrupt = frames
                .last()
                .unwrap()
                .actions
                .iter()
                .filter(|(_, actor, age)| {
                    *actor == owner
                        && !interrupted
                        && (mode == "interrupt" && *age >= early_interrupt_at
                            || mode == "cross_interrupt"
                                && (first_contact_at..traversal_at + traversal_duration)
                                    .contains(age)
                                && battle.actors()[target.index()].hp < hp
                                && battle.actors()[owner.index()].movement.forward > 0.)
                })
                .map(|(id, _, _)| *id)
                .collect::<Vec<_>>();
            let interrupting_now = !interrupt.is_empty();
            interrupted |= interrupting_now;
            frames.push(battle.step(BattleInput {
                interrupt,
                ..Default::default()
            })?);
            if expecting_hits
                && pass_end_separation.is_none()
                && frames
                    .last()
                    .unwrap()
                    .actions
                    .iter()
                    .any(|(_, actor, age)| {
                        *actor == owner && *age >= traversal_at + traversal_duration + 1
                    })
            {
                pass_end_separation = Some(
                    (battle.actors()[owner.index()].position[0]
                        - battle.actors()[target.index()].position[0])
                        * direction,
                );
            }
            if interrupting_now {
                assert_eq!(
                    battle.actors()[owner.index()].movement.forward,
                    0.,
                    "interrupting the action must stop its traversal immediately"
                );
            }
            if battle.activity(owner) == resonance_battle::Activity::Idle {
                break;
            }
        }
        assert_eq!(interrupted, matches!(mode, "interrupt" | "cross_interrupt"));
        assert_eq!(battle.activity(owner), resonance_battle::Activity::Idle);
        assert!(battle.actors()[owner.index()].position[0].abs() <= traversal_limit + 0.001);
        let contacts = battle.take_fixture_contact_damage();
        let hits = contacts
            .iter()
            .filter(|c| c.owner_id == owner && c.target_id == target)
            .collect::<Vec<_>>();
        eprintln!(
            "Ghost Wolf {mode} at {initial_distance}: {} contacts, HP {hp} -> {}, final position {:?}, pass clearance {pass_end_separation:?}",
            hits.len(),
            battle.actors()[2].hp,
            battle.actors()[0].position
        );
        if expecting_hits && hits.len() != 2 {
            missed_close_passes.push((mode, initial_distance, hits.len()));
            continue;
        }
        if expecting_hits {
            assert!(
                frames.len() <= 62,
                "Ghost Wolf must return control within 62 updates, took {}",
                frames.len()
            );
            let clearance = pass_end_separation.context("traversal endpoint not observed")?;
            assert!(
                (60.0..=85.0).contains(&clearance),
                "close-range traversal must stop within second-sword reach: {clearance}"
            );
            assert_eq!(
                hits.len(),
                2,
                "Ghost Wolf must land its passing slash and finishing cut"
            );
            assert!(battle.actors()[2].hp < hp);
            assert!(
                hits[0].owner.movement.forward > 0.,
                "the opening slash must connect during traversal"
            );
            assert!((hits[1].owner.position[0] - hits[1].target.position[0]) * direction > 0.);
            assert!(
                hits[1].owner.facing_direction[2].abs() > 0.1,
                "the finishing sweep must connect while Lloyd is turning"
            );
            let hit_updates = frames
                .iter()
                .filter(|frame| {
                    frame.cues.iter().any(|cue| {
                        matches!(cue, Cue::Hit { owner: hit_owner, actor, .. }
                            if *hit_owner == owner && *actor == target)
                    })
                })
                .filter_map(|frame| {
                    frame.actions.iter().find_map(|(_, actor, age)| {
                        // Completed frames report the age of the next update.
                        (*actor == owner).then_some(age.saturating_sub(1))
                    })
                })
                .collect::<Vec<_>>();
            let turn_updates = authored
                .attack
                .events
                .iter()
                .filter_map(|(at, event)| {
                    matches!(event, resonance_content::battle_normal::Event::Turn(_))
                        .then_some(u32::from(*at))
                })
                .collect::<Vec<_>>();
            assert_eq!(hit_updates.len(), 2);
            let turn_start = *turn_updates
                .iter()
                .min()
                .context("Ghost Wolf turn missing")?;
            let turn_end = *turn_updates
                .iter()
                .max()
                .context("Ghost Wolf turn missing")?;
            assert!(
                (turn_start..turn_end).contains(&hit_updates[1]),
                "the finishing contact must occur before the turn completes"
            );
            assert!(
                battle.actors()[0].facing_direction[0] * direction < -0.99,
                "Lloyd must finish facing the enemy behind him"
            );
        } else if mode == "cross_interrupt" {
            assert_eq!(
                hits.len(),
                1,
                "interrupting the pass must cancel the finishing cut"
            );
            assert_eq!(battle.actors()[0].movement.forward, 0.);
        } else if mode == "away" {
            assert!(
                hits.len() <= 1,
                "only the return cut may reach a target behind Lloyd"
            );
            assert_eq!(battle.actors()[owner.index()].position, [0.; 3]);
        } else if mode == "outside" {
            // The ordinary sword contacts still run; only traversal is out of range.
            assert_eq!(battle.actors()[owner.index()].position, [0.; 3]);
        } else {
            assert!(hits.is_empty());
            assert_eq!(battle.actors()[2].hp, hp);
            if mode == "far" {
                assert_eq!(battle.actors()[owner.index()].position, [0.; 3]);
            }
        }
        assert!(
            frames
                .iter()
                .flat_map(|f| &f.cues)
                .any(|cue| matches!(cue, Cue::Notice { actor, .. } if *actor == owner))
        );
        let cried = frames.iter().flat_map(|frame| &frame.cues).any(|cue| {
            matches!(cue, Cue::Voice { actor, sound, .. }
                if *actor == owner && *sound == expected_voice)
        });
        assert_eq!(
            cried,
            mode != "interrupt",
            "an early interruption must also cancel the pending voice"
        );
        assert!(!fixture.diagnostics.has_errors());
        assert!(!battle.is_diagnostic());
        if expecting_hits {
            let position = battle.actors()[owner.index()].position[0];
            let mut control = resonance_battle::ControlInput::neutral(owner);
            control.stick = [(80. * direction) as i8, 0];
            battle.step(BattleInput::from(control))?;
            assert!(
                (battle.actors()[owner.index()].position[0] - position) * direction > 0.,
                "movement input must work immediately after Ghost Wolf finishes"
            );
        }
    }
    assert!(
        missed_close_passes.is_empty(),
        "close-range passes missed: {missed_close_passes:?}"
    );
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn ghost_wolf_ability_plus_chains_lightning_tiger_into_sword_rain_alpha() -> Result<()> {
    use resonance_battle::{Activity, BattleInput, ButtonInput, ControlInput, Cue};
    let fixture = GhostWolfFixture::load(&[1], true)?;
    for initial_distance in [60., 80., 100.] {
        let mut battle = fixture.battle(0)?;
        assert!(battle.actors()[0].equipment.combo_traits.ability_plus);
        let (owner, target) =
            fixture.place(&mut battle, [0., 0., 0.], [initial_distance, 0., 0.])?;
        let ghost_wolf = battle
            .prepared_technique(owner, TechniqueId(260))
            .context("Ghost Wolf unavailable")?
            .action;
        let lightning_tiger = battle
            .prepared_technique(owner, TechniqueId(254))
            .context("Lightning Tiger unavailable")?
            .action;
        let alpha = battle
            .prepared_technique(owner, TechniqueId(14))
            .context("Sword Rain: Alpha unavailable")?
            .action;
        let initial_tp = battle.actors()[0].tp;
        let mut started = Vec::new();
        let mut hits = Vec::new();
        let mut age = 0;
        let mut queued_stage = 0;
        for tick in 0..400 {
            let mut control = ControlInput::neutral(owner);
            if tick == 0 || (age >= 5 && queued_stage < started.len() && started.len() < 6) {
                let stage = started.len();
                if stage < 3 {
                    control.attack = ButtonInput {
                        pressed: true,
                        held: true,
                        released: false,
                    };
                    // Three forward inputs alternate the native low and neutral slashes.
                    control.stick = [80, 0];
                } else {
                    control.assigned_arte = Some(TechniqueId(match stage {
                        3 => 260,
                        4 => 254,
                        _ => 14,
                    }));
                }
                queued_stage = stage;
            }
            let frame = battle.step(BattleInput::from(control))?;
            for cue in &frame.cues {
                match cue {
                    Cue::Started {
                        actor,
                        definition: Some(action),
                        ..
                    } if *actor == owner => started.push((tick, *action)),
                    Cue::Hit {
                        owner: source,
                        actor,
                        ..
                    } if *source == owner && *actor == target => {
                        hits.push((
                            tick,
                            started.len(),
                            battle.actors()[target.index()].reaction.combo_hits,
                        ));
                        assert_eq!(
                            battle.actors()[target.index()].reaction.combo_hits as usize,
                            hits.len(),
                            "the target must remain in one continuous hit combo: {hits:?}"
                        );
                    }
                    _ => {}
                }
            }
            age = frame
                .actions
                .iter()
                .find_map(|(_, actor, age)| (*actor == owner).then_some(*age))
                .unwrap_or_default();
            if started.len() == 6 && battle.activity(owner) == Activity::Idle {
                break;
            }
        }
        eprintln!(
            "Ghost Wolf Ability Plus combo at {initial_distance}: starts {started:?}; hits {hits:?}"
        );
        assert_eq!(started.len(), 6, "three normals and three chained artes");
        assert_eq!(started[3].1, ghost_wolf);
        assert_eq!(started[4].1, lightning_tiger);
        assert_eq!(started[5].1, alpha);
        assert_eq!(
            hits.iter().map(|(_, stage, _)| *stage).collect::<Vec<_>>(),
            [1, 2, 3, 4, 4, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6]
        );
        assert_eq!(battle.technique_uses(owner, TechniqueId(260)), Some(1));
        assert_eq!(battle.technique_uses(owner, TechniqueId(254)), Some(1));
        assert_eq!(battle.technique_uses(owner, TechniqueId(14)), Some(1));
        assert_eq!(battle.actors()[0].tp, initial_tp - 36);
        assert_eq!(battle.activity(owner), Activity::Idle);
        assert!(!fixture.diagnostics.has_errors());
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current cooked encounter assets; CPU only"]
fn ghost_wolf_preserves_its_half_turn_when_the_arena_blocks_the_pass() -> Result<()> {
    use resonance_battle::{ActionRequest, Activity, BattleInput, Cue};
    let fixture = GhostWolfFixture::load(&[1], false)?;
    // A clear pass, a shortened pass, and a pass blocked before crossing the target.
    for sign in [1_f32, -1.] {
        for target_radius in [120_f32, 800., 885.] {
            let mut battle = fixture.battle(1234)?;
            let (owner, target) = fixture.place(
                &mut battle,
                [sign * (target_radius - 120.), 0., 0.],
                [sign * target_radius, 0., 0.],
            )?;
            let action = battle
                .prepared_technique(owner, TechniqueId(260))
                .context("Ghost Wolf unavailable")?
                .action;
            let mut hits = 0;
            let mut turned = false;
            let mut boundary_contact = false;
            let mut idle_updates = 0;
            for tick in 0..246 {
                let input = if tick == 0 {
                    BattleInput::from(ActionRequest {
                        actor: owner,
                        target,
                        action,
                    })
                } else {
                    BattleInput::default()
                };
                let frame = battle.step(input)?;
                turned |= frame
                    .actions
                    .iter()
                    .any(|(_, actor, age)| *actor == owner && *age > 45);
                let lloyd = &battle.actors()[owner.index()];
                assert!(lloyd.position[0].hypot(lloyd.position[2]) <= 850.001);
                boundary_contact |=
                    lloyd.movement.steering.arena_contact() != resonance_battle::ArenaContact::None;
                if turned {
                    assert!(
                        lloyd.facing_direction[0] * sign < -0.999
                            && lloyd.facing_direction[2].abs() < 0.001
                            && lloyd.heading.to_radians().sin() * sign < -0.999,
                        "completed half-turn changed at edge {sign} * {target_radius}, update {tick}: heading {}, facing {:?}",
                        lloyd.heading,
                        lloyd.facing_direction
                    );
                }
                hits += frame
                    .cues
                    .iter()
                    .filter(|cue| {
                        matches!(cue, Cue::Hit { owner: source, actor, .. }
                            if *source == owner && *actor == target)
                    })
                    .count();
                if turned && battle.activity(owner) == Activity::Idle {
                    idle_updates += 1;
                    if idle_updates == 30 {
                        break;
                    }
                }
            }
            assert_eq!(
                idle_updates, 30,
                "recovery must finish and retain facing at idle"
            );
            assert_eq!(
                hits, 2,
                "both cuts must still connect at edge {sign} * {target_radius}"
            );
            assert_eq!(boundary_contact, target_radius > 780.);
            assert_eq!(
                battle.actors()[owner.index()].position[0] * sign > target_radius,
                target_radius < 850.,
                "the boundary fixture must exercise both crossed and uncrossed targets"
            );
            assert!(!battle.is_diagnostic());
        }
    }
    assert!(!fixture.diagnostics.has_errors());
    Ok(())
}
