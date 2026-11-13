//! Exercise the Luin shopkeeper through the field menu service.
use super::*;
use resonance_events::input::Button;

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn luin_crafting_returns_to_the_shopkeeper_and_restores_control() -> Result<()> {
    let mut field = enter(MARTEL_START, 461, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    assert!(field.events.interact(310)?);
    field.advance_until(|f| f.crafting.as_ref().is_some_and(|c| c.fade == 0))?;
    assert!(!field.crafting.as_ref().unwrap().recipes().is_empty());
    assert!(!field.player_has_control());
    assert!(field.checkpoint().is_err());
    field.step(FieldInput {
        pressed_buttons: [Button::Cancel].into(),
        ..Default::default()
    })?;
    let mut saw_farewell = false;
    field.replay(|f| {
        saw_farewell |= !f.events.world.dialogue.is_empty();
        Ok(f.player_has_control())
    })?;
    assert!(
        saw_farewell,
        "closing must resume the shopkeeper's dialogue"
    );
    assert!(field.crafting.is_none());
    assert!(field.events.exploration_error.is_none());
    Ok(())
}
