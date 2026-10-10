use super::*;
use resonance_content::arte;
use std::collections::BTreeMap;

#[test]
fn technique_metadata_shares_identity_and_validates_player_and_ai_ranges() -> Result<()> {
    let mut catalogue = arte::Catalogue {
        definitions: vec![arte::Definition::default(); 35],
        learning: vec![vec![1, 2, 34]],
    };
    let actions = BTreeMap::from([(1, resonance_battle::ActionKey(0))]);
    for (uses_weapon_reach, maximum, ai_minimum) in
        [(true, 50., 0.), (false, 800., 700.), (false, 8000., 500.)]
    {
        catalogue.definitions[1].capabilities = arte::TechniqueCapabilities {
            target: arte::TechniqueTarget::Enemy,
            uses_weapon_reach,
            ..Default::default()
        };
        catalogue.definitions[1].action_range = maximum;
        catalogue.definitions[1].element = 3;
        let rows = battle::control::techniques(&catalogue, 1, &actions)?;
        assert_eq!(rows.len(), 1);
        let row = rows[0];
        assert_eq!(
            (row.catalogue, row.action, row.element),
            (1, resonance_battle::ActionKey(0), 3)
        );
        assert_eq!(row.player_range, [0., maximum]);
        assert_eq!(row.ai_range, [ai_minimum, maximum]);
        assert_eq!(row.capabilities.uses_weapon_reach, uses_weapon_reach);
    }
    let guard = battle::control::techniques(
        &catalogue,
        1,
        &BTreeMap::from([(34, resonance_battle::ActionKey(1))]),
    )?[0];
    assert_eq!(guard.player_range, guard.ai_range);
    assert_eq!(
        guard.capabilities.family,
        Some(resonance_battle::ArteFamily::Arcane)
    );
    assert!(!guard.capabilities.uses_weapon_reach);
    assert_eq!(guard.player_range, [0.; 2]);
    assert_eq!(guard.capabilities.target, arte::TechniqueTarget::SelfTarget);
    assert!(battle::control::techniques(&catalogue, 1, &BTreeMap::new())?.is_empty());
    assert!(battle::control::techniques(&catalogue, 2, &actions).is_err());
    assert!(
        battle::control::techniques(
            &catalogue,
            1,
            &BTreeMap::from([(99, resonance_battle::ActionKey(0))])
        )
        .is_err()
    );
    for maximum in [-1., 0., f32::NAN, f32::INFINITY] {
        catalogue.definitions[1].action_range = maximum;
        assert!(battle::control::techniques(&catalogue, 1, &actions).is_err());
    }
    catalogue.definitions[1].action_range = 100.;
    catalogue.definitions[1].capabilities.target = arte::TechniqueTarget::Unavailable;
    assert!(battle::control::techniques(&catalogue, 1, &actions).is_err());
    catalogue.definitions.truncate(1);
    assert!(battle::control::techniques(&catalogue, 1, &actions).is_err());
    Ok(())
}
