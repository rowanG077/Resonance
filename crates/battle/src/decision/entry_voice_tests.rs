use super::*;
use crate::{ActorAvailability, BattleInput, ResourceBinding, SoundBinding, VoiceLine};
use std::{collections::BTreeMap, sync::Arc};

fn definition() -> EntryVoiceDefinition {
    EntryVoiceDefinition {
        action: 99,
        repeated_formation: false,
        major_enemy: false,
        enemy_count: 1,
        level_difference: 1,
    }
}

fn prepare(voice: EntryVoiceDefinition, unavailable: &[usize]) -> Result<PreparedBattle> {
    let mut actors: Vec<_> = (0..3 + usize::from(voice.enemy_count))
        .map(|index| {
            let mut actor = crate::tests::actor(if index < 3 { Side::Party } else { Side::Enemy });
            actor.position = [index as f32 * 50., 0., 0.];
            // An unavailable actor can retain positive HP; 10A8 reads the
            // lifecycle state, not the healing-recipient eligibility predicate.
            if unavailable.contains(&index) {
                actor.availability = ActorAvailability::Absent;
            }
            actor
        })
        .collect();
    actors[0].position[0] = -50.;
    let sources = BTreeMap::from([(
        "battle::entry_voice".into(),
        include_str!("../../../../scripts/battle/entry_voice.sym").into(),
    )]);
    let compiled = symphonia_script_compiler::compile(
        "battle::entry_voice",
        &sources,
        &crate::native_declarations(),
    )?;
    let resources = compiled
        .assets
        .iter()
        .map(|asset| {
            let relative: u16 = asset.path.rsplit('/').next().unwrap().parse().unwrap();
            ResourceBinding::Voice(
                actors
                    .iter()
                    .enumerate()
                    .map(|(index, actor)| {
                        (actor.side == Side::Party).then(|| VoiceLine {
                            sound: SoundBinding {
                                resource: 1,
                                index: relative + [1, 121, 241][index],
                            },
                            duration: 1,
                        })
                    })
                    .collect(),
            )
        })
        .collect();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == "battle::entry_voice::initialize")
        .unwrap()
        .entry;
    let choices = (0..actors.len())
        .map(|index| EntryChoice {
            actor: ActorId(index as u8),
            strategy: 1,
            action: None,
            idle_ticks: 0,
            idle_variation: 0,
        })
        .collect();
    PreparedBattle::new(
        actors,
        vec![crate::ActionDefinition {
            id: 99,
            phase: ActionPhase::Decision,
            program: Arc::new(compiled.program),
            entry,
            duration: 0,
            tp_cost: 0,
            resources,
        }],
        1_559_683_463,
        vec![],
        vec![],
    )? // Original draw88: every actor initialized.
    .with_entry_voice(voice)?
    .with_entry_choices(choices)
}

#[test]
fn opening_selector_retains_genis_257_and_exact_rng_for_first_actor_visit() -> Result<()> {
    let prepared = prepare(definition(), &[])?;
    assert_eq!(prepared.random_seed, 2_738_554_465); // Original draw90 / camera P0.
    assert_eq!(
        prepared.voices[2].pending,
        Some((
            SoundBinding {
                resource: 1,
                index: 257
            },
            1
        ))
    );
    assert!(prepared.voices[2].centered);
    assert!(
        prepared.voices[..2]
            .iter()
            .all(|voice| voice.pending.is_none())
    );
    let mut battle = Battle::new(Arc::new(prepared));
    let frame = battle.step(BattleInput::default())?;
    assert!(matches!(
        frame.cues.as_slice(),
        [Cue::Voice {
            actor: ActorId(2),
            sound: SoundBinding { index: 257, .. },
            centered: true,
            ..
        }]
    ));
    assert!(!battle.voices[2].centered);
    Ok(())
}

#[test]
fn entry_selector_preserves_source_branch_precedence_and_draw_counts() -> Result<()> {
    for (repeat, major, enemies, difference, relative) in [
        (true, true, 5, -3, 23),
        (false, true, 5, -3, 19),
        (false, false, 5, -3, 20),
        (false, false, 1, -3, 22),
        (false, false, 1, 3, 21),
    ] {
        let prepared = prepare(
            EntryVoiceDefinition {
                repeated_formation: repeat,
                major_enemy: major,
                enemy_count: enemies,
                level_difference: difference,
                ..definition()
            },
            &[],
        )?;
        assert_eq!(prepared.random_seed, 2_203_188_994); // Only participant draw89.
        assert_eq!(prepared.voices[2].pending.unwrap().0.index, 241 + relative);
    }
    Ok(())
}

#[test]
fn unavailable_random_participant_falls_back_without_an_extra_draw() -> Result<()> {
    let prepared = prepare(definition(), &[2])?;
    assert_eq!(prepared.random_seed, 2_738_554_465);
    assert_eq!(prepared.voices[0].pending.unwrap().0.index, 17);
    assert!(prepared.voices[2].pending.is_none());
    let all_unavailable = prepare(definition(), &[0, 1, 2])?;
    assert_eq!(all_unavailable.voices[2].pending.unwrap().0.index, 257);
    Ok(())
}
