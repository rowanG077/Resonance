use super::*;
use resonance_content::diagnostics::Diagnostics;

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
