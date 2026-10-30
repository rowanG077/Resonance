use super::*;
use serde_json::json;

fn attacks() -> Result<[Attack; 7]> {
    let attack: Attack = serde_json::from_value(json!({
        "continuation":{"directions":14,"fallback":4},
        "duration":34,"chain_at":19,"recovery":10,"reach":[120,0],
        "events":[[0,{"pose":{"clip":"slot-30","rate":0.0,"blend":4}}],
            [12,{"contact":{"duration":11,"groups":[0],"shape":{"kind":"box"},
                "radius":30.0,"height":30.0,"hit":{
                    "kind":"slash","power":"normal","element":"inherited",
                    "guard_pressure":1,"hitstun":30,"cooldown":120,"stun_chance":0,
                    "stagger":2,"armor_damage":1,"recoil":1}}}],
            [12,{"projectile":{"member":2}}]]
    }))?;
    Ok(std::array::from_fn(|_| attack.clone()))
}

#[test]
fn named_roles_and_shared_hit_edits_preserve_timelines_and_explicit_values() -> Result<()> {
    let original = attacks()?;
    let mut document = Character::from_attacks(original.clone())?;
    assert_eq!(document.hits.len(), 1);
    assert!(matches!(
        document.attacks[&Role::AerialThrust],
        Track::Shared(Role::Neutral)
    ));
    let text = serde_json::to_string(&document)?;
    assert!(text.contains(r#""directions":["rising","thrust","low"]"#));
    assert!(text.contains(r#""fallback":"finisher""#));
    assert!(text.contains(r#""rate":0.0"#));
    let decoded: Character = serde_json::from_str(&text)?;
    assert_eq!(
        serde_json::to_value(decoded.prepare()?)?,
        serde_json::to_value(&original)?
    );

    document.hits.get_mut("neutral").unwrap().power = crate::battle_action::Power::Fixed(0);
    let changed = document.prepare()?;
    for attack in &changed {
        let Event::Contact(contact) = &attack.events[1].1 else {
            panic!()
        };
        assert_eq!(contact.hit.power, crate::battle_action::Power::Fixed(0));
        let Event::Projectile { hit, .. } = &attack.events[2].1 else {
            panic!()
        };
        assert!(
            hit.is_none(),
            "absent projectile hit must keep the inherited recipe"
        );
        assert_eq!(attack.continuation, original[0].continuation);
    }
    // An inline replacement deliberately stops sharing with the local hit recipe.
    let Track::Inline(mut isolated) = document.attacks[&Role::Neutral].clone() else {
        panic!()
    };
    isolated.events[1].1 = map_hits(isolated.events[1].1.clone(), |_| {
        Ok(Hit::Inline(ActionHit {
            power: crate::battle_action::Power::Percent(250),
            ..document.hits["neutral"]
        }))
    })?;
    document
        .attacks
        .insert(Role::AerialThrust, Track::Inline(isolated));
    let isolated = document.prepare()?;
    let Event::Contact(contact) = &isolated[6].events[1].1 else {
        panic!()
    };
    assert_eq!(contact.hit.power, crate::battle_action::Power::Percent(250));
    let Event::Contact(contact) = &isolated[0].events[1].1 else {
        panic!()
    };
    assert_eq!(contact.hit.power, crate::battle_action::Power::Fixed(0));
    Ok(())
}

#[test]
fn named_normals_reject_missing_roles_hits_chained_aliases_and_old_arrays() -> Result<()> {
    let document = Character::from_attacks(attacks()?)?;
    let mut missing = document.clone();
    missing.hits.clear();
    assert!(
        missing
            .prepare()
            .unwrap_err()
            .to_string()
            .contains("missing hit recipe")
    );
    let mut missing = document.clone();
    missing.attacks.remove(&Role::Rising);
    assert!(missing.prepare().is_err());
    let mut chained = document.clone();
    chained
        .attacks
        .insert(Role::Rising, Track::Shared(Role::Thrust));
    assert!(
        chained
            .prepare()
            .unwrap_err()
            .to_string()
            .contains("directly")
    );
    let mut cyclic = document.clone();
    cyclic
        .attacks
        .insert(Role::Neutral, Track::Shared(Role::Neutral));
    assert!(cyclic.prepare().is_err());
    assert!(
        Choices::try_from(Continuation {
            directions: 128,
            fallback: None
        })
        .is_err()
    );
    assert!(
        Choices::try_from(Continuation {
            directions: 0,
            fallback: Some(7)
        })
        .is_err()
    );
    assert!(serde_json::from_value::<Choices>(json!({"directions":["up"]})).is_err());
    assert!(
        serde_json::from_value::<NormalAttacks>(serde_json::to_value(vec![attacks()?])?).is_err()
    );
    Ok(())
}

#[test]
fn character_mapping_names_are_editable_and_do_not_select_runtime_slots() -> Result<()> {
    let character = Character::from_attacks(attacks()?)?;
    let mut document = NormalAttacks {
        mappings: Mappings::new((1..=9).map(|id| (id, format!("custom-{id}"))).collect())?,
        characters: (1..=9)
            .map(|id| (format!("custom-{id}"), character.clone()))
            .collect(),
    };
    assert_eq!(document.clone().prepare()?.len(), 9);
    document.characters.remove("custom-4");
    assert!(
        document
            .clone()
            .prepare()
            .unwrap_err()
            .to_string()
            .contains("character 4")
    );
    document.mappings = Mappings::new([(1, "custom-1".into())].into())?;
    assert!(document.prepare().is_err());
    Ok(())
}
