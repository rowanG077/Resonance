use super::{
    effect_runtime::{battle_from_actor, start},
    nurse_trails::{Visit, assert_particle_state},
};
use anyhow::{Context, Result};
use resonance_battle::{BattleInput, Cue};
use resonance_content::battle_effect::{Record, SourceBank};
use resonance_game::battle::effect_program;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Deserialize)]
struct Command {
    tick: u32,
    owner: u32,
    effect_age: u32,
    record: String,
    random_before: u32,
    random_after: u32,
    origin_bits: [u32; 3],
    heading_bits: u32,
    visits: Vec<Visit>,
}

#[derive(Deserialize)]
struct Fixture {
    commands: Vec<Command>,
}

fn source() -> Result<SourceBank> {
    #[derive(Deserialize)]
    struct ModelFixture {
        bank: SourceBank,
    }
    Ok(serde_json::from_str::<ModelFixture>(include_str!(
        "../fixtures/nurse-model-particles.json"
    ))?
    .bank)
}

#[test]
fn nurse_recipient_modifiers_and_particle_motion_match_all_original_instances() -> Result<()> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../fixtures/nurse-recipient-particles.json"))?;
    let bank = source()?;
    let mut updates = 0;
    for command in &fixture.commands {
        let bytes = (0..command.record.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&command.record[i..i + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let record = Record::from_bytes(bytes.try_into().ok().context("invalid fixture record")?);
        let mut input = bank.program(6)?;
        // The ring modifier retains a 30-degree step in float scratch3.
        // Execute its preceding visits too; they consume no randomness.
        // Spark modifiers overwrite their scratch on every emission.
        let count = if record.command == 2 {
            command.effect_age / 2 + 1
        } else {
            1
        };
        input.records = vec![record; count as usize];
        input.records.push(Record {
            age: 1,
            command: 254,
            argument: 0,
            operand: 0,
        });
        let effect = effect_program::prepare(&input, 77, 6, &mut super::effect_runtime::no_sound)?;
        let mut actor = super::actor();
        // Component boundary: supply the original emission center/heading and
        // RNG state. Keep this actor stationary; moving center attachments have
        // their own regression. Subsequent observed actor movement is not replayed.
        actor.position = command.origin_bits.map(f32::from_bits);
        actor.heading = f32::from_bits(command.heading_bits);
        let mut run = battle_from_actor(
            "battle::show_on(effect, 6, battle::owner()); battle::finish();",
            BTreeMap::from([(6, Arc::new(effect))]),
            command.random_before,
            actor,
        );
        let mut frame = start(&mut run);
        let id = frame.particles.last().context("missing particle")?.id;
        assert_eq!(run.0.random_state(), command.random_after);
        for (age, visit) in command.visits.iter().enumerate() {
            if age > 0 {
                frame = run.0.step(BattleInput::default())?;
            }
            let particle = frame
                .particles
                .iter()
                .find(|p| p.id == id)
                .context("early expiry")?;
            assert_eq!(particle.age, age as i16);
            assert_eq!(particle.member, u16::from(record.command));
            assert_eq!(
                visit.combat_tick,
                command.visits[0].combat_tick + age as u32
            );
            assert_eq!(particle.origin.map(f32::to_bits), command.origin_bits);
            assert_particle_state(particle, visit)?;
            assert_eq!(visit.random_before, visit.random_after);
            updates += 1;
        }
        let first_visit = command.tick + u32::from(command.effect_age == 0);
        assert_eq!(command.visits[0].combat_tick, first_visit);
        let expired = run.0.step(BattleInput::default())?;
        assert!(expired.particles.iter().all(|p| p.id != id));
        assert!(
            expired
                .cues
                .contains(&Cue::ParticleExpired { particle: id })
        );
        assert_eq!(run.0.random_state(), command.random_after);
    }
    assert_eq!(fixture.commands.len(), 51);
    assert_eq!(updates, 2091);
    eprintln!(
        "Nurse recipients: 51 original emissions, {updates} particle visits; exact modifier RNG and motion bits"
    );
    Ok(())
}

#[test]
fn maintained_recovery_emits_on_each_eligible_recipient_before_healing() -> Result<()> {
    use resonance_battle::{
        ActionDefinition, ActionPhase, ActionRequest, Battle, EffectBank, PreparedBattle,
        ResourceBinding, Side, SoundBinding,
    };
    let compiled = symphonia_script_compiler::compile(
        "battle::nurse",
        &BTreeMap::from([(
            "battle::nurse".into(),
            include_str!("../../../../scripts/battle/nurse.sym").into(),
        )]),
        &resonance_battle::native_declarations(),
    )?;
    let resources = compiled
        .assets
        .iter()
        .map(|asset| match asset.kind.as_str() {
            "battle::Effect" => ResourceBinding::Effect(77),
            "battle::Voice" => ResourceBinding::Voice(vec![
                Some(resonance_battle::VoiceLine {
                    sound: SoundBinding {
                        resource: 1,
                        index: 43
                    },
                    duration: 0
                });
                5
            ]),
            "battle::ActorTints" => super::actor_tints(),
            kind => panic!("unexpected fixture asset {kind}"),
        })
        .collect();
    let entry = compiled
        .program
        .authored()
        .context("missing authored module")?
        .functions
        .iter()
        .find(|f| f.name == "battle::nurse::recover")
        .context("missing recovery entry")?
        .entry;
    let effect = effect_program::prepare(&source()?.program(6)?, 77, 6, &mut |index| {
        assert_eq!(index, 104);
        Ok(SoundBinding { resource: 1, index })
    })?;
    let mut party = vec![super::actor(); 4];
    for (i, actor) in party.iter_mut().enumerate() {
        actor.position = [i as f32 * 100., 0., 0.];
        actor.body.center_offset = [0., 80., 0.];
    }
    party[1].effect_scale = 0.5;
    party[1].hp = 90; // Report the nominal amount, even at the HP cap.
    party[2].hp = 0;
    party[3].petrified = true;
    let mut enemy = super::actor();
    enemy.side = Side::Enemy;
    party.push(enemy);
    let prepared = Arc::new(PreparedBattle::new(
        party,
        vec![ActionDefinition {
            id: 99,
            phase: ActionPhase::Resident,
            program: Arc::new(compiled.program),
            entry,
            duration: 250,
            tp_cost: 0,
            resources,
        }],
        1,
        vec![],
        vec![EffectBank {
            resource: 77,
            models: Default::default(),
            members: BTreeMap::from([(6, Arc::new(effect))]),
        }],
    )?);
    let actors: Vec<_> = prepared.actor_ids().collect();
    let mut battle = Battle::new(prepared);
    battle.step(BattleInput {
        actions: vec![ActionRequest {
            actor: actors[0],
            target: actors[0],
            action: 99,
        }],
        ..Default::default()
    })?;
    for _ in 0..120 {
        let frame = battle.step(BattleInput::default())?;
        assert!(frame.particles.is_empty());
        assert!(
            !frame
                .cues
                .iter()
                .any(|c| matches!(c, Cue::Recovered { .. }))
        );
    }
    let heal = battle.step(BattleInput::default())?;
    assert!(heal.particles.is_empty()); // Resident callbacks follow the particle group.
    assert_eq!(heal.cues.len(), 6);
    for (slot, cues) in heal.cues.chunks_exact(3).enumerate() {
        let point = [slot as f32 * 100., 80., 0.];
        assert!(
            matches!(cues[0], Cue::Effect { resource: 77, member: 6, position, .. } if position == point)
        );
        assert!(
            matches!(cues[1], Cue::Sound { sound: SoundBinding { index: 104, .. }, position, .. } if position == point)
        );
        assert!(
            matches!(cues[2], Cue::Recovered { actor, nominal: 40, applied } if actor == actors[slot] && applied == if slot == 0 {40} else {10})
        );
    }
    let shown = battle.step(BattleInput::default())?;
    assert_eq!(shown.particles.len(), 6);
    for (slot, &actor) in actors.iter().take(2).enumerate() {
        let glow = shown
            .particles
            .iter()
            .find(|p| p.owner == actor && p.member == 3)
            .context("missing recipient glow")?;
        assert_eq!(glow.draw_after, Some(actor));
        assert_eq!(glow.origin, [slot as f32 * 100., 80., 0.]);
        let resonance_battle::ParticleGeometry::Size {
            value, velocity, ..
        } = glow.state.geometry
        else {
            panic!("expected radial glow")
        };
        let scale = if slot == 0 { 1. } else { 0.5 };
        assert_eq!(value, [0., 0., 104. * scale]);
        assert_eq!(velocity, [0., 0., 7.75 * scale]);
    }
    let held = battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(held.particles, shown.particles);
    assert!(held.cues.is_empty());
    let observed: Fixture =
        serde_json::from_str(include_str!("../fixtures/nurse-recipient-particles.json"))?;
    let first_owner = observed.commands[0].owner;
    let mut frame = shown;
    let mut emissions = 0;
    for elapsed in 1..=74 {
        if elapsed > 1 {
            frame = battle.step(BattleInput::default())?;
        }
        let expected = observed
            .commands
            .iter()
            .filter(|c| c.owner == first_owner && c.visits[0].combat_tick == 498 + elapsed)
            .map(|c| u16::from_str_radix(&c.record[4..6], 16))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for &actor in actors.iter().take(2) {
            let actual: Vec<_> = frame
                .particles
                .iter()
                .filter(|p| p.owner == actor && p.age == 0)
                .map(|p| p.member)
                .collect();
            assert_eq!(actual, expected, "recipient effect at +{elapsed}");
            emissions += actual.len();
        }
    }
    assert_eq!(emissions, 34);
    assert!(frame.particles.is_empty());
    assert_eq!(
        battle.actors().iter().map(|a| a.hp).collect::<Vec<_>>(),
        [50, 100, 0, 10, 10]
    );
    Ok(())
}
