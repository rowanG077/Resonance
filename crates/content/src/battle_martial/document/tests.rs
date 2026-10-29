use super::*;
use serde_json::json;

fn member() -> Result<Member> {
    let hit = json!({"kind":"slash","power":"normal","element":"inherited","guard_pressure":1,
        "hitstun":30,"cooldown":60,"stun_chance":0,"stagger":2,"recoil":1});
    let contact = json!({"contact":{"duration":10,"groups":[0],"shape":{"kind":"box"},"radius":30.0,"height":40.0,"hit":hit}});
    let shot = json!({"projectile":{"member":2,"hit":hit,"feedback":{"effect":3,"sound":0,"on_guard":false}}});
    let pose = json!({"clip":"idle","blend":0,"rate":0.0});
    let attack: Attack = serde_json::from_value(json!({
        "duration":50,"recovery":10,"events":[[0,contact],[0,shot],[3,{"projectile":{"member":3}}]],
        "callback_hits":{"1":hit},"pose_sequence":[{"start":{"at":0},"pose":pose}],
        "flight":{"gravity_at":10,"gravity":-1.0,"finish_pose":pose},
        "hammer":{"pattern":{"kind":"single","alternate_events":[[2,contact],[4,shot]]},"hits":[hit,hit,hit],"impact_effect":1,"impact_sound":2},
        "steal":{"attempt_at":10,"bonus_at":15,"success_duration":60,"success_events":[[3,contact]],
            "bonus_hit":hit,"bonus_offset":[0.0,0.0,0.0],"bonus_forward_speed":1.0,"bonus_velocity_jitter":[0.0,0.0,0.0]},
        "fierce_fang":{"hits":[hit,hit]},"hurricane_hit":hit
    }))?;
    Ok(Member {
        variants: [
            (0, SceneAction::Prepared(Box::new(attack.clone()))),
            (1, SceneAction::Prepared(Box::new(attack))),
            (
                2,
                SceneAction::Unsupported {
                    reason: "unresolved authored command".into(),
                },
            ),
        ]
        .into(),
        unsupported_reason: None,
    })
}

#[test]
fn local_contacts_expand_main_hammer_and_theft_tracks_and_keep_overrides() -> Result<()> {
    let original = member()?;
    let mut named = Technique::from_member(
        original.clone(),
        [
            (0, "lloyd".into()),
            (1, "zelos".into()),
            (2, "unavailable".into()),
        ]
        .into(),
    )?;
    assert_eq!(named.contacts.len(), 1);
    assert_eq!(named.projectiles.len(), 2);
    let serialized = serde_json::to_string(&named)?;
    assert!(!serialized.contains("\"status\""));
    let roundtrip: Technique = serde_json::from_str(&serialized)?;
    assert_eq!(
        serde_json::to_value(roundtrip.prepare()?)?,
        serde_json::to_value(original)?
    );
    named.contacts.get_mut("strike").unwrap().hit.power = crate::battle_action::Power::Fixed(0);
    named.projectiles.get_mut("projectile").unwrap().hit = None;
    let prepared = named.prepare()?;
    for id in [0, 1] {
        let attack = prepared.variants[&id].prepared()?;
        let mut contacts = 0;
        map_events(attack.clone(), |event| {
            match &event {
                Command::Contact(contact) => {
                    assert_eq!(contact.hit.power, crate::battle_action::Power::Fixed(0));
                    contacts += 1;
                }
                Command::Projectile { hit, .. } => assert!(hit.is_none()),
                _ => {}
            }
            Ok(event)
        })?;
        assert_eq!(contacts, 3);
        assert_eq!(
            attack.callback_hits[&1].power,
            crate::battle_action::Power::Normal
        );
    }
    assert!(prepared.variants[&2].prepared().is_err());
    named.contacts.clear();
    assert!(
        named
            .prepare()
            .unwrap_err()
            .to_string()
            .contains("missing contact")
    );
    Ok(())
}

#[test]
fn named_techniques_preserve_id_bindings_and_require_existing_variants() -> Result<()> {
    let original = member()?;
    let technique = Technique::from_member(
        original,
        [
            (0, "first".into()),
            (1, "second".into()),
            (2, "failed".into()),
        ]
        .into(),
    )?;
    let mut catalogue = Catalogue {
        mappings: Mappings::new([(17, "my-technique".into())].into())?,
        bindings: [(
            41,
            [(
                6,
                Binding {
                    member: 17,
                    variant: 1,
                },
            )]
            .into(),
        )]
        .into(),
        techniques: [("my-technique".into(), technique)].into(),
    };
    assert_eq!(catalogue.clone().prepare()?.attack(41, 6)?.duration, 50);
    assert!(
        catalogue
            .clone()
            .prepare()?
            .variant(Binding {
                member: 17,
                variant: 3
            })
            .is_err()
    );
    catalogue
        .techniques
        .get_mut("my-technique")
        .unwrap()
        .variants
        .remove("second");
    assert!(format!("{:#}", catalogue.prepare().unwrap_err()).contains("missing variant second"));
    assert!(serde_json::from_value::<Catalogue>(json!({"members":{},"bindings":{}})).is_err());
    Ok(())
}

#[test]
fn secondary_tracks_share_repeated_events_but_keep_distinct_contacts_inline() -> Result<()> {
    let mut original = member()?;
    original.variants.retain(|id, _| *id == 0);
    let SceneAction::Prepared(attack) = original.variants.get_mut(&0).unwrap() else {
        panic!()
    };
    let HammerPattern::Single { alternate_events } = &mut attack.hammer.as_mut().unwrap().pattern
    else {
        panic!()
    };
    let Command::Contact(contact) = &mut alternate_events[0].1 else {
        panic!()
    };
    contact.radius += 1.0;
    let named = Technique::from_member(original.clone(), [(0, "lloyd".into())].into())?;
    assert_eq!(
        named.contacts.len(),
        1,
        "main and theft contacts share strike"
    );
    assert_eq!(
        named.projectiles.len(),
        1,
        "main and hammer shots share projectile"
    );
    let Variant::Prepared(attack) = &named.variants["lloyd"] else {
        panic!()
    };
    let HammerPattern::Single { alternate_events } = &attack.hammer.as_ref().unwrap().pattern
    else {
        panic!()
    };
    assert!(matches!(
        alternate_events[0].1,
        Event::Command(Command::Contact(_))
    ));
    assert!(matches!(
        alternate_events[1].1,
        Event::Reference(Reference::Projectile(_))
    ));
    assert!(matches!(
        attack.steal.as_ref().unwrap().success_events[0].1,
        Event::Reference(Reference::Contact(_))
    ));
    assert_eq!(
        serde_json::to_value(named.prepare()?)?,
        serde_json::to_value(original)?
    );
    Ok(())
}
