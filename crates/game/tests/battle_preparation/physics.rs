//! Exercise combo physics through published actions, models and ordinary inputs.
//! GC reference receipts are retained in tools/oracle/fixtures/battle-physics.json.
use super::*;
use resonance_battle::{Activity, ActorId, Control, ControlInput, Cue, TechniqueId};
use resonance_content::{diagnostics::Diagnostics, session::SessionData};

struct Fixture {
    files: Files,
    session: SessionData,
}

impl Fixture {
    fn load() -> Result<Self> {
        let files = resonance_content::preparation::capture(
            &resonance_content::library::Library::open(&common::asset_root(), &[])?,
            &[],
            || false,
            Diagnostics::new(true),
        )?;
        let session = SessionData::load(&files)?;
        Ok(Self { files, session })
    }

    fn battle(&self, character: u8, learned: &[u16], mode: Control) -> Result<FixtureBattle> {
        self.battle_with_recipient(character, learned, mode, |_| {})
    }

    fn battle_with_recipient(
        &self,
        character: u8,
        learned: &[u16],
        mode: Control,
        configure: impl Fn(&mut Actor),
    ) -> Result<FixtureBattle> {
        self.battle_with_loadout(character, learned, mode, 1, [0; 4], configure)
    }

    fn battle_with_loadout(
        &self,
        character: u8,
        learned: &[u16],
        mode: Control,
        formation: u16,
        skills: [u8; 4],
        configure: impl Fn(&mut Actor),
    ) -> Result<FixtureBattle> {
        let menus = self.session.rules.as_ref().context("missing rules")?;
        let mut party = resonance_events::party::Party::new(&self.session, Default::default())?;
        party.formation = vec![character];
        party.field_leader = character;
        party.settings.battle_controls = [0; 4];
        let member = &mut party.members[usize::from(character - 1)];
        // Keep learning transitions from replacing an input during these loops.
        let catalogue = resonance_content::arte::Catalogue::load(&self.files)?;
        let character_kind = battle::party::Character::try_from(character)?;
        member.techniques.extend(
            catalogue
                .learned_by(character)?
                .iter()
                .copied()
                .filter(|&arte| battle::martial::technique(character_kind, arte).is_some()),
        );
        member.techniques.extend(learned);
        member.shortcuts = [learned[0]; 4];
        member.ex_skills = skills;
        member.ex_gems = skills.map(|skill| if skill == 0 { 0 } else { 4 });
        member.compound_ex_skills.clear();
        member.overlimit = 0;
        member.base_stats[0] = 9000;
        member.base_stats[1] = 500;
        member.hp = 9000;
        member.tp = 500;
        let assets = battle::encounter::Assets::capture(
            &mut self.files.library().context("missing library")?.snapshot(),
            menus,
            &self.session,
            &party,
            opening_setup(
                formation,
                resonance_events::battle::DefeatPolicy::ResumeEvent,
            ),
            self.files.diagnostics().clone(),
            &|| false,
        )?;
        let prepared = assets.prepare_fixture(
            menus,
            opening_options(1),
            |_| Ok(None),
            |actors| {
                for (index, (actor, setup)) in actors.iter_mut().enumerate() {
                    actor.control = if index == 0 { mode } else { Control::Enemy };
                    if index != 0 {
                        // Manual control suppresses choices while preserving authored behavior.
                        setup.recovery_return = None;
                        actor.equipment.max_hp = 30000;
                        actor.hp = 30000;
                        configure(actor);
                    }
                }
                Ok(())
            },
        )?;
        let mut battle = FixtureBattle(prepared.core);
        for index in 1..battle.actors().len() {
            battle.set_fixture_neutral_control(ActorId::from_index(index)?)?;
        }
        battle.enter()?;
        battle.restore_fixture_placement(ActorId::from_index(0)?, [0., 0., 0.], Some(90.))?;
        battle.restore_fixture_placement(ActorId::from_index(1)?, [110., 0., 0.], Some(-90.))?;
        Ok(battle)
    }
}

/// The published frame owns requests emitted after contact resolution. Consume
/// them exactly as the ordinary game host does, before the next physics step.
/// Dropping these requests leaves hurt poses stale and changes collision geometry.
struct FixtureBattle(Battle);

impl std::ops::Deref for FixtureBattle {
    type Target = Battle;

    fn deref(&self) -> &Battle {
        &self.0
    }
}

impl std::ops::DerefMut for FixtureBattle {
    fn deref_mut(&mut self) -> &mut Battle {
        &mut self.0
    }
}

impl FixtureBattle {
    fn step(&mut self, input: BattleInput) -> Result<resonance_battle::BattleFrame> {
        let mut frame = self.0.step(input)?;
        let clock = frame.clock;
        self.0.advance_models(&mut frame, clock, true)?;
        Ok(frame)
    }

    fn enter(&mut self) -> Result<()> {
        for _ in 0..600 {
            let frame = self.step(BattleInput::default())?;
            if self.phase() == resonance_battle::BattlePhase::Combat
                && !frame.clock.paused()
                && !self.is_paused()
            {
                return Ok(());
            }
        }
        bail!("battle entry did not enable actor control")
    }
}

fn buttons(attack: bool, technique: bool, guard: bool) -> BattleInput {
    let mut input = ControlInput::neutral(ActorId::from_index(0).unwrap());
    input.attack.pressed = attack;
    input.attack.held = attack;
    input.technique.pressed = technique;
    input.technique.held = technique;
    input.guard.held = guard;
    BattleInput::from(input)
}

fn cast_after_normals(battle: &mut FixtureBattle, count: u8) -> Result<usize> {
    let owner = ActorId::from_index(0)?;
    let target = ActorId::from_index(1)?;
    let mut input = buttons(true, false, false);
    let mut normals = 0;
    let mut hits = 0;
    for _ in 0..300 {
        let frame = battle.step(input)?;
        input = BattleInput::default();
        hits += frame
            .cues
            .iter()
            .filter(|cue| {
                matches!(cue,
            Cue::Hit { actor, result, .. } if *actor == target && !result.suppresses_reaction())
            })
            .count();
        if normals < count
            && frame.cues.iter().any(|cue| {
                matches!(cue,
            Cue::Started { actor, .. } if *actor == owner)
            })
        {
            normals += 1;
            input = buttons(normals < count, normals == count, false);
        }
        if battle.casting_remaining(owner).is_some() {
            assert_eq!(normals, count);
            return Ok(hits);
        }
    }
    bail!(
        "normal chain never reached its buffered spell: admitted {normals}/{count}, owner {:?}, target {:?}",
        battle.activity(owner),
        battle.activity(target)
    )
}

#[test]
#[ignore = "requires current named battle assets; CPU only"]
fn published_spells_cancel_normal_recovery_and_allow_another_normal_chain() -> Result<()> {
    let fixture = Fixture::load()?;
    for (character, spell, mode, full_chain) in [
        (6, 213, Control::Manual, false),
        (6, 221, Control::SemiAuto, true),
        (9, 213, Control::SemiAuto, true),
        (9, 221, Control::Manual, false),
    ] {
        let mut battle = fixture.battle(character, &[spell], mode)?;
        let owner = ActorId::from_index(0)?;
        let count = if full_chain {
            battle.actors()[0].equipment.normal_combo_limit
        } else {
            1
        };
        let tp = battle.actors()[0].tp;
        let uses = battle.technique_uses(owner, TechniqueId(spell));
        assert_eq!(uses, Some(0));
        let mut total_hits = 0;
        for cycle in 0..4 {
            total_hits += cast_after_normals(&mut battle, count).with_context(|| format!("character {character}, spell {spell}, {mode:?}, full chain {full_chain}, cycle {cycle}"))?;
            // Admission initializes the chant; the next callback reads Guard.
            let frame = battle.step(buttons(false, false, true))?;
            assert_eq!(battle.activity(owner), Activity::Idle);
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
            assert_eq!(
                battle.actors()[0].tp,
                (tp + total_hits as u16).min(battle.actors()[0].equipment.max_tp)
            );
            assert_eq!(battle.technique_uses(owner, TechniqueId(spell)), uses);
            assert_eq!(
                total_hits,
                usize::from(count) * (cycle + 1),
                "a spell-cancel cycle missed a normal"
            );
            assert_eq!(
                battle.actors()[1].reaction.combo_hits,
                total_hits as i32,
                "spell cancel dropped the received combo"
            );
        }
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[ignore = "requires current named battle assets; CPU only"]
fn published_demon_fang_picks_up_lloyd_knockdowns() -> Result<()> {
    let fixture = Fixture::load()?;
    let owner = ActorId::from_index(0)?;
    let target = ActorId::from_index(1)?;
    // Cornered Twin Tiger Blade and Hunting Beast retain pickup windows
    // of 38 and 29 updates when Lloyd regains control.
    for (arte, expected_window, fang) in [(9, 38, 1), (9, 38, 2), (9, 38, 3), (22, 29, 1)] {
        let mut battle = fixture.battle(1, &[arte, fang], Control::Manual)?;
        battle.restore_fixture_placement(owner, [650., 0., 0.], Some(90.))?;
        battle.restore_fixture_placement(target, [760., 0., 0.], Some(-90.))?;
        let (hit, before, window) = first_pickup_contact(&mut battle, arte, fang, false)?;
        assert_eq!(window, expected_window);
        assert!(!hit.suppresses_reaction(), "arte {arte}, fang {fang}");
        assert_eq!(battle.activity(target), Activity::Hurt);
        assert_eq!(battle.actors()[1].reaction.combo_hits, before + 1);
        assert_eq!(
            battle.actors()[1].reaction.recoil.kind,
            resonance_battle::RecoilKind::Normal
        );
    }
    Ok(())
}

fn assigned(technique: u16) -> BattleInput {
    BattleInput::from(ControlInput {
        assigned_arte: Some(TechniqueId(technique)),
        ..ControlInput::neutral(ActorId::from_index(0).unwrap())
    })
}

/// Count contacts independently of the recipient's displayed combo. A later
/// successful hit must not hide a recovery between two otherwise valid cycles.
fn continuous_step(
    battle: &mut FixtureBattle,
    input: BattleInput,
    hits: &mut i32,
) -> Result<resonance_battle::BattleFrame> {
    let frame = battle.step(input)?;
    *hits += frame
        .cues
        .iter()
        .filter(|cue| {
            matches!(cue,
        Cue::Hit { actor, result, .. }
        if actor.index() == 1 && !result.suppresses_reaction())
        })
        .count() as i32;
    assert_eq!(battle.actors()[1].reaction.combo_hits, *hits);
    Ok(frame)
}

fn first_pickup_contact(
    battle: &mut FixtureBattle,
    arte: u16,
    fang: u16,
    chain: bool,
) -> Result<(resonance_battle::HitResult, i32, u8)> {
    let owner = ActorId::from_index(0)?;
    let target = ActorId::from_index(1)?;
    let opening = battle
        .prepared_technique(owner, TechniqueId(arte))
        .unwrap()
        .action;
    let follow = battle
        .prepared_technique(owner, TechniqueId(fang))
        .unwrap()
        .action;
    let mut hits = 0;
    let mut frame = continuous_step(battle, assigned(arte), &mut hits)?;
    for _ in 0..240 {
        if frame.cues.iter().any(|cue| {
            matches!(cue,
            Cue::Started { actor, definition: Some(action), .. }
            if *actor == owner && *action == opening)
        }) {
            break;
        }
        frame = continuous_step(battle, BattleInput::default(), &mut hits)?;
    }
    assert!(frame.cues.iter().any(|cue| matches!(cue,
        Cue::Started { actor, definition: Some(action), .. }
        if *actor == owner && *action == opening)));
    if !chain {
        for _ in 0..240 {
            continuous_step(battle, BattleInput::default(), &mut hits)?;
            if battle.activity(owner) == Activity::Idle {
                break;
            }
        }
        assert_eq!(battle.activity(owner), Activity::Idle);
        assert_eq!(battle.activity(target), Activity::KnockedDown);
    }
    let window = battle.actors()[1].reaction.stagger.window;
    let mut frame = continuous_step(battle, assigned(fang), &mut hits)?;
    let mut follow_started = false;
    for _ in 0..240 {
        follow_started |= frame.cues.iter().any(|cue| {
            matches!(cue,
            Cue::Started { actor, definition: Some(action), .. }
            if *actor == owner && *action == follow)
        });
        if chain {
            assert_ne!(
                battle.activity(owner),
                Activity::Idle,
                "chained arte fell through idle"
            );
        }
        let before = battle.activity(target);
        let before_hits = hits;
        frame = continuous_step(battle, BattleInput::default(), &mut hits)?;
        if let Some(hit) = frame.cues.iter().find_map(|cue| match cue {
            Cue::Hit {
                actor,
                source: resonance_battle::ContactSource::Projectile(_),
                result,
                ..
            } if *actor == target => Some(*result),
            _ => None,
        }) {
            assert!(
                follow_started,
                "projectile did not belong to the admitted follow-up"
            );
            assert_eq!(
                before,
                Activity::KnockedDown,
                "ordinary hit mistaken for a pickup"
            );
            assert_eq!(
                before_hits,
                match arte {
                    20 => 2,
                    22 => 3,
                    _ => 4,
                }
            );
            assert!(!battle.is_diagnostic());
            return Ok((hit, before_hits, window));
        }
    }
    bail!("follow-up projectile never contacted its recipient")
}

#[test]
#[ignore = "requires current named battle assets; CPU only"]
fn published_beast_pickup_uses_an_early_demonic_chaos_chain() -> Result<()> {
    let fixture = Fixture::load()?;
    // Retain both tight pickups and their late/open negatives. Demonic Chaos
    // can replace the end of Beast before recovery completes.
    for (mode, corner, distance, chain, pickup) in [
        (Control::Manual, true, 110., true, true),
        (Control::SemiAuto, true, 170., true, true),
        (Control::Manual, true, 110., false, false),
        (Control::SemiAuto, false, 110., true, false),
    ] {
        let mut battle = fixture.battle(1, &[20, 3], mode)?;
        let origin = if corner { 650. } else { 0. };
        battle.restore_fixture_placement(ActorId::from_index(0)?, [origin, 0., 0.], Some(90.))?;
        battle.restore_fixture_placement(
            ActorId::from_index(1)?,
            [origin + distance, 0., 0.],
            Some(-90.),
        )?;
        let (hit, before, _) = first_pickup_contact(&mut battle, 20, 3, chain)?;
        assert_eq!(
            !hit.suppresses_reaction(),
            pickup,
            "{mode:?}, corner={corner}, distance={distance}, chain={chain}"
        );
        assert_eq!(
            battle.actors()[1].reaction.combo_hits,
            before + i32::from(pickup)
        );
        assert_eq!(
            battle.activity(ActorId::from_index(1)?),
            if pickup {
                Activity::Hurt
            } else {
                Activity::KnockedDown
            }
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires current named battle assets; CPU only"]
fn published_late_pickups_preserve_reaction_poses_and_enemy_model_parts() -> Result<()> {
    let fixture = Fixture::load()?;
    // Retain reaction poses and Sword Dancer's hidden first-stage parts;
    // stale geometry can manufacture pickups that the original game rejects.
    for (mode, arte, formation, follow_up, corner) in [
        (Control::Manual, 20, 1, false, true),
        (Control::SemiAuto, 20, 1, true, true),
        (Control::SemiAuto, 9, 1, false, false),
        (Control::Manual, 9, 1, true, false),
        (Control::Manual, 9, 87, false, false),
        (Control::SemiAuto, 9, 462, false, false),
    ] {
        let skills = [
            if follow_up {
                resonance_content::menu_data::ex_effect::FOLLOW_UP
            } else {
                0
            },
            0,
            0,
            0,
        ];
        let mut battle =
            fixture.battle_with_loadout(1, &[arte, 1], mode, formation, skills, |_| {})?;
        assert_eq!(
            battle.actors()[0].equipment.reaction_ex.follow_up,
            follow_up
        );
        if corner {
            battle.restore_fixture_placement(ActorId::from_index(0)?, [650., 0., 0.], Some(90.))?;
            battle.restore_fixture_placement(
                ActorId::from_index(1)?,
                [760., 0., 0.],
                Some(-90.),
            )?;
        }
        let (hit, before, _) = first_pickup_contact(&mut battle, arte, 1, false)?;
        assert_eq!(
            hit.protection,
            resonance_battle::HitProtection::Reduced,
            "{mode:?}, arte={arte}, formation={formation}, follow_up={follow_up}"
        );
        assert_eq!(
            battle.activity(ActorId::from_index(1)?),
            Activity::KnockedDown
        );
        assert_eq!(battle.actors()[1].reaction.combo_hits, before);
        assert_eq!(battle.actors()[1].reaction.stagger.window, 0);
    }
    Ok(())
}

#[test]
#[ignore = "requires current named battle assets; CPU only"]
fn published_lloyd_pickup_loops_preserve_the_combo_and_exhaust_the_window() -> Result<()> {
    let fixture = Fixture::load()?;
    let owner = ActorId::from_index(0)?;
    let target = ActorId::from_index(1)?;
    // Isolate window exhaustion with one route per control mode and arena edge.
    for (arte, mode, side) in [(9, Control::Manual, -1.), (22, Control::SemiAuto, 1.)] {
        let mut battle = fixture.battle_with_recipient(1, &[arte, 1], mode, |actor| {
            actor.reaction.stun.resistance = u8::MAX;
        })?;
        // Placement preserves physical facing. Turn through ordinary
        // input before moving the idle actors to either arena edge.
        battle.step(BattleInput::from(ControlInput {
            stick: [(side * 80.) as i8, 0],
            ..ControlInput::neutral(owner)
        }))?;
        for _ in 0..40 {
            battle.step(BattleInput::default())?;
        }
        battle.restore_fixture_placement(owner, [side * 650., 0., 0.], Some(side * 90.))?;
        battle.restore_fixture_placement(target, [side * 760., 0., 0.], Some(-side * 90.))?;
        let mut hits = 0;
        continuous_step(&mut battle, assigned(arte), &mut hits)?;
        let arcane = battle
            .prepared_technique(owner, TechniqueId(arte))
            .unwrap()
            .action;
        let mut windows = Vec::new();
        let mut exhausted = false;
        for cycle in 0..24 {
            let mut started = 0;
            for _ in 0..240 {
                let frame = continuous_step(&mut battle, BattleInput::default(), &mut hits)?;
                started += frame
                    .cues
                    .iter()
                    .filter(|cue| {
                        matches!(cue,
                            Cue::Started { actor, definition: Some(action), .. }
                            if *actor == owner && *action == arcane)
                    })
                    .count();
                if battle.activity(owner) == Activity::Idle {
                    break;
                }
            }
            assert_eq!(
                started, 1,
                "queued arcane arte did not execute exactly once"
            );
            assert_eq!(battle.activity(owner), Activity::Idle);
            assert_eq!(
                battle.activity(target),
                Activity::KnockedDown,
                "arte={arte}, {mode:?}, side={side}, cycle={cycle}, hits={hits}, positions={:?}",
                battle
                    .actors()
                    .iter()
                    .map(|actor| actor.position)
                    .collect::<Vec<_>>()
            );
            assert!(
                battle.actors()[0].tp >= 4,
                "TP exhaustion masked window exhaustion"
            );
            windows.push(battle.actors()[1].reaction.stagger.window);
            let before = hits;
            continuous_step(&mut battle, assigned(1), &mut hits)?;
            continuous_step(&mut battle, BattleInput::default(), &mut hits)?;
            continuous_step(&mut battle, assigned(arte), &mut hits)?;
            let mut contact = None;
            for _ in 0..120 {
                let frame = continuous_step(&mut battle, BattleInput::default(), &mut hits)?;
                contact = frame.cues.iter().find_map(|cue| match cue {
                    Cue::Hit { actor, result, .. } if *actor == target => Some(*result),
                    _ => None,
                });
                if contact.is_some() {
                    break;
                }
            }
            let hit = contact.context("Demon Fang missed instead of testing the window")?;
            if hit.suppresses_reaction() {
                assert!(cycle >= 3, "short established pickup route regressed");
                assert_eq!(hit.protection, resonance_battle::HitProtection::Reduced);
                assert_eq!(battle.activity(target), Activity::KnockedDown);
                assert_eq!(battle.actors()[1].reaction.stagger.window, 0);
                assert_eq!(hits, before);
                exhausted = true;
                break;
            }
            assert_eq!(battle.activity(target), Activity::Hurt);
            assert_eq!(hits, before + 1);
        }
        assert!(exhausted, "route never reached its pickup limit");
        assert!(windows.windows(2).all(|pair| pair[1] < pair[0]));
        eprintln!(
            "pickup limit: arte={arte}, {mode:?}, side={side}, hits={hits}, idle windows={windows:?}"
        );
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}
