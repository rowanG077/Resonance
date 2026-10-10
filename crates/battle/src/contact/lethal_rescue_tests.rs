use super::*;
use crate::{Activity, ActorAvailability, Power};
use std::sync::Arc;

#[test]
fn rescued_contact_keeps_the_target_and_awards_no_kill_rewards() -> Result<()> {
    let mut victim = crate::tests::actor(Side::Enemy);
    victim.hp = 1;
    victim.equipment.recovery.lethal.angel_tear = true;
    victim.overlimit = crate::OverLimit::new(501).unwrap();
    let candidate = crate::lethal_rescue::tests::prepared(victim, 19);
    let mut battle = candidate.finish()?;
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(false));
    let mut definition = crate::tests::contact_projectile(false, false);
    definition.velocity = [0.; 3];
    definition.acceleration = [0.; 3];
    definition.offset = [0.; 3];
    definition.active = None;
    definition.contact.as_mut().unwrap().hit.power = Power::Fixed(54);
    battle.emit(
        Arc::new(definition),
        ActionId(1),
        ActorId(0),
        ActorId(1),
        [0.; 3],
    )?;
    let birth = battle.step(crate::BattleInput::default())?;
    assert!(!birth.projectiles[0].contact_active);
    assert_eq!(battle.actors[1].hp, 1);
    let cues = battle.step(crate::BattleInput::default())?.cues;
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Hit {
            actor: ActorId(1),
            ..
        }
    )));
    assert_eq!(battle.actors[1].hp, 25);
    assert_eq!(battle.actors[1].availability, ActorAvailability::Active);
    assert_ne!(battle.activity(ActorId(1)), Activity::Defeated);
    assert_eq!(battle.actors[1].overlimit.charge(), 501);
    assert_eq!(battle.ledger.kills, [0, 0]);
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(1)));
    assert_eq!(battle.ledger.deaths, [0, 0]);
    assert!(!battle.angel_tear_armed(ActorId(1))?);
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Rescued {
            kind: crate::RescueKind::AngelTear,
            ..
        }
    )));
    assert!(!battle.is_diagnostic());
    assert!(!cues.iter().any(|cue| matches!(cue, Cue::Defeated { .. })));
    assert!(battle.timed_hold_remaining().is_some());
    Ok(())
}
