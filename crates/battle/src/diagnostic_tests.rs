use super::*;
use resonance_content::diagnostics::Diagnostics;

#[test]
fn tolerant_actions_report_multiple_faults_and_keep_other_actors_running() {
    let source = r#"
        asset sound: battle::Sound = "missing";
        pub task missing_actor() { battle::ally(99); }
        pub task missing_sound() { battle::sound(sound, 1); }
        pub task healing() {
            await battle::at_age(ticks(2));
            battle::heal_percent(battle::owner(), 10);
        }
    "#;
    let mut prepared = prepared_entry(
        source,
        vec![
            actor(Side::Party),
            actor(Side::Party),
            actor(Side::Party),
            actor(Side::Enemy),
        ],
        20,
        "missing_actor",
    );
    let prepared_mut = Arc::get_mut(&mut prepared).unwrap();
    let template = prepared_mut.actions[0].clone();
    prepared_mut.actions = ["missing_actor", "missing_sound", "healing"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let mut action = template.clone();
            action.id = 99 + index as u16;
            action.entry = action
                .program
                .authored()
                .unwrap()
                .functions
                .iter()
                .find(|function| function.name == format!("test::{name}"))
                .unwrap()
                .entry;
            action
        })
        .collect();
    let input = || BattleInput {
        actions: (0..3)
            .map(|index| ActionRequest {
                actor: ActorId(index),
                target: ActorId(index),
                action: 99 + u16::from(index),
            })
            .collect(),
        ..Default::default()
    };
    let diagnostics = Diagnostics::new(false);
    let mut battle = Battle::new(prepared.clone());
    battle.set_diagnostics(diagnostics.clone());
    let frame = battle.step(input()).unwrap();
    assert_eq!(frame.update, 1);
    assert_eq!(diagnostics.entries().len(), 2);
    assert!(battle.is_diagnostic());
    assert!(!battle.ended);
    assert_eq!(battle.sequences.len(), 1);
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Interrupted { .. }))
            .count(),
        2
    );
    for _ in 0..5 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.actors[2].hp, 60);
    assert_eq!(diagnostics.entries().len(), 2);
    assert!(
        diagnostics
            .entries()
            .iter()
            .all(|entry| entry.occurrences == 1)
    );

    let mut strict = Battle::new(prepared);
    assert!(
        strict
            .step(input())
            .unwrap_err()
            .to_string()
            .contains("invalid battle roster index")
    );
    assert!(strict.ended);
}

#[test]
fn tolerant_effect_emissions_skip_missing_members_and_finish_the_action() {
    let source = r#"
        asset bank: battle::Effect = "test/effect";
        pub task run() {
            battle::show(bank, 9, battle::owner());
            battle::show(bank, 10, battle::owner());
            battle::show(bank, 6, battle::owner());
            battle::heal_percent(battle::owner(), 10);
        }
    "#;
    let diagnostics = Diagnostics::new(false);
    let mut battle = Battle::new(prepared(
        source,
        vec![actor(Side::Party), actor(Side::Enemy)],
        20,
    ));
    battle.set_diagnostics(diagnostics.clone());
    let frame = battle.step(request(0)).unwrap();
    assert_eq!(diagnostics.entries().len(), 2);
    assert!(
        frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Effect { member: 6, .. }))
    );
    assert_eq!(battle.actors[0].hp, 60);
    assert!(battle.is_diagnostic());
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(diagnostics.entries().len(), 2);
}

#[test]
fn tolerant_projectile_failure_retires_only_that_projectile() {
    let mut invalid = contact_projectile(true, true);
    invalid.contact.as_mut().unwrap().radius_growth = f32::MAX;
    let mut battle = clash_battle(invalid, contact_projectile(false, true), [0.; 3]);
    let diagnostics = Diagnostics::new(false);
    battle.set_diagnostics(diagnostics.clone());
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.projectiles.len(), 1);
    assert_eq!(frame.projectiles[0].id, ProjectileId(2));
    assert_eq!(frame.projectiles[0].age, 1);
    assert!(frame.cues.contains(&Cue::ProjectileExpired {
        projectile: ProjectileId(1)
    }));
    assert_eq!(diagnostics.entries().len(), 1);
    assert!(battle.is_diagnostic());
    battle.step(BattleInput::default()).unwrap();
}

#[test]
fn presentation_diagnostics_do_not_taint_the_simulation() {
    let diagnostics = Diagnostics::new(false);
    diagnostics
        .report("test renderer", anyhow::anyhow!("missing texture"))
        .unwrap();
    let mut battle = Battle::new(prepared("pub task run() {}", vec![actor(Side::Party)], 1));
    battle.set_diagnostics(diagnostics);
    battle.step(BattleInput::default()).unwrap();
    assert!(!battle.is_diagnostic());
}
