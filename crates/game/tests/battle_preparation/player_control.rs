use super::*;
use resonance_content::arte;
use resonance_events::party::Member;
use std::collections::BTreeMap;

#[test]
fn player_shortcuts_require_learned_ready_actions_and_ignore_ai_disable_preferences() -> Result<()>
{
    let mut files = Files::default();
    let mut slots = vec![0; 40];
    slots[..2].copy_from_slice(&[1, 2]);
    let mut catalogue = arte::Catalogue {
        definitions: vec![arte::Definition::default(); 3],
        learning: vec![arte::LearningList {
            count: 2,
            technique_slots: slots,
        }],
        combinations: vec![],
        learning_storage: [0; 5],
    };
    // Original Demon Fang1: flags41106, signed range word38=800, TP4.
    catalogue.definitions[1].flags = 0x41106;
    catalogue.definitions[1].tp_cost = 4;
    catalogue.definitions[1].recovery_ticks = 800;
    files
        .bytes
        .insert(arte::PATH.into(), serde_json::to_vec(&catalogue)?.into());
    let mut member: Member = serde_json::from_value(serde_json::json!({
        "affinity": 0, "level": 1, "experience": 0, "base_stats": [100, 20, 30, 40, 50, 60, 70],
        "hp": 100, "tp": 20, "conditions": 0, "luck": 50, "overlimit": 0,
        "equipment": [0, 0, 0, 0, 0, 0], "techniques": [1], "shortcuts": [1, 0, 2, 1],
        "disabled_techniques": [1],
    }))?;
    let actions = BTreeMap::from([(1, 91)]);
    let shortcuts = battle::control::shortcuts(&files, 1, &member, &actions)?;
    for slot in [0, 3] {
        let shortcut = shortcuts[slot].unwrap();
        assert_eq!(
            (shortcut.action, shortcut.minimum, shortcut.maximum),
            (91, 0., 800.)
        );
    }
    assert!(shortcuts[1].is_none() && shortcuts[2].is_none());
    assert!(battle::control::shortcuts(&files, 1, &member, &BTreeMap::new()).is_err());
    member.techniques.clear();
    assert!(
        battle::control::shortcuts(&files, 1, &member, &BTreeMap::new())?
            .iter()
            .all(Option::is_none)
    );
    member.techniques.insert(1);
    catalogue.definitions[1].recovery_ticks = 1000;
    files
        .bytes
        .insert(arte::PATH.into(), serde_json::to_vec(&catalogue)?.into());
    let shortcut = battle::control::shortcuts(&files, 1, &member, &actions)?[0].unwrap();
    assert_eq!((shortcut.minimum, shortcut.maximum), (0., 8000.));
    catalogue.definitions[1].cast_time_adjustment = -1;
    files
        .bytes
        .insert(arte::PATH.into(), serde_json::to_vec(&catalogue)?.into());
    assert!(battle::control::shortcuts(&files, 1, &member, &actions).is_err());
    Ok(())
}
