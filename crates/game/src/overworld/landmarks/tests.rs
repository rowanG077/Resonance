use super::*;

pub(crate) fn definitions() -> Landmarks {
    Landmarks {
        worlds: std::array::from_fn(|world| {
            (1..=if world == 0 { 98 } else { 81 })
                .map(|id| Landmark {
                    id: (world * 256 + id) as u16,
                    position: [10000. + id as f32 * 200., 10000.],
                    height: None,
                    radius: 80.,
                    interaction: Interaction::Disabled,
                    marker: Marker::None,
                    ship_camera_clearance: None,
                    flight_floor: None,
                    name: format!("Place {id}"),
                    map_listed: false,
                    visit_alias: None,
                    shops: Vec::new(),
                    shop_variants: Vec::new(),
                    automatic: false,
                    entry_overrides: Vec::new(),
                    reward_item: None,
                    required_character: (world == 0 && id == 63)
                        .then_some(resonance_content::overworld::PartyCharacter::Genis),
                    guidepost: None,
                })
                .collect()
        }),
    }
}
pub(crate) use symphonia_constants::GlobalId as Global;

#[derive(Default)]
pub(crate) struct TestProgress {
    memory: symphonia_script_vm::Memory,
    items: BTreeMap<u16, u8>,
    visited: BTreeSet<u16>,
    event_flags: BTreeSet<u16>,
    formation: Vec<u8>,
}
impl TestProgress {
    pub fn set(&mut self, global: Global, value: i32) {
        self.memory
            .write(0x40 + global.raw() * 4, symphonia_script::Width::S32, value)
            .unwrap();
    }
    pub fn view(&self) -> Progress<'_> {
        static STATE: symphonia_script::authored::ScriptState = BTreeMap::new();
        Progress {
            memory: &self.memory,
            items: &self.items,
            visited: &self.visited,
            event_flags: &self.event_flags,
            formation: &self.formation,
            script_state: &STATE,
        }
    }
}
pub(crate) fn progress() -> TestProgress {
    TestProgress {
        formation: vec![1, 3],
        ..Default::default()
    }
}

#[test]
fn flight_zones_respect_activation_bounds_and_existing_height() -> Result<()> {
    let mut data = definitions();
    data.worlds[0][0].position = [10., 1000.];
    data.worlds[0][0].interaction = Interaction::Active;
    data.worlds[0][0].flight_floor = Some(resonance_content::overworld::FlightFloor {
        margin: 20.,
        height: 500.,
        active_only: true,
    });
    let mut locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    let zones = [FlightZone {
        position: [2000., 1000.],
        radius: 300.,
        height: 1000.,
    }];
    // Landmark bounds include the world seam.
    for (x, ground, expected) in [
        (-90., 0., 500.),
        (-91., 0., 0.),
        (110., 0., 500.),
        (111., 0., 0.),
        (10., 800., 800.),
        (1700., 0., 1000.),
        (1699., 0., 0.),
    ] {
        assert_eq!(
            locations.flight_floor(
                World::Sylvarant,
                Position::from_map([x, 1000., ground])?,
                &zones
            )?,
            expected
        );
    }
    locations.appearances.get_mut(&1).unwrap().interaction = Interaction::Disabled;
    let center = Position::from_map([10., 1000., 0.])?;
    assert_eq!(
        locations.flight_floor(World::Sylvarant, center, &zones)?,
        0.
    );
    Arc::make_mut(&mut locations.definitions).worlds[0][0]
        .flight_floor
        .as_mut()
        .unwrap()
        .active_only = false;
    assert_eq!(
        locations.flight_floor(World::Sylvarant, center, &zones)?,
        500.
    );
    assert_eq!(
        locations.flight_floor(World::TetheAlla, center, &zones)?,
        0.
    );
    Ok(())
}

#[test]
fn caravan_camps_follow_story_quests_and_key_items_for_drawing_and_contact() -> Result<()> {
    let camps = [53, 54, 55, 57, 58, 60];
    let mut data = definitions();
    for id in camps {
        // The catalogue exposes every camp; story rules select the active stops.
        data.worlds[0][id - 1].interaction = Interaction::Active;
        data.worlds[0][id - 1].marker = Marker::Model(11);
    }
    let mut locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    let mut check = |p: &TestProgress, expected: &[u16]| -> Result<()> {
        locations.refresh(&p.view())?;
        let visible: Vec<_> = locations
            .visible(World::Sylvarant)
            .filter_map(|(l, _)| camps.contains(&usize::from(l.id)).then_some(l.id))
            .collect();
        assert_eq!(
            visible,
            expected,
            "story {}",
            resonance_events::script_global(&p.memory, Global::Story as i32)?
        );
        for id in camps {
            let l = locations.definition(id as u16).unwrap();
            let contact = locations.contact(
                World::Sylvarant,
                Position::from_map([l.position[0], l.position[1], 0.])?,
                Mount::Foot,
                0.,
            );
            assert_eq!(
                contact.map(|c| c.id),
                expected.contains(&(id as u16)).then_some(id as u16)
            );
        }
        Ok(())
    };
    let mut p = progress();
    for (story, expected) in [
        (902_999, &[][..]),
        (903_000, &[53][..]),
        (1_000_999, &[53][..]),
        (1_001_000, &[][..]),
        (1_402_999, &[][..]),
        (1_403_000, &[55, 60][..]),
        (13_200_999, &[55, 60][..]),
        (13_201_000, &[58][..]),
        (14_000_000, &[58][..]),
    ] {
        p.set(Global::Story, story);
        check(&p, expected)?;
    }
    p.set(Global::LinkiteTree, 40_000);
    check(&p, &[])?;
    p.event_flags = [symphonia_constants::StoryFlag::DevilsArmAcquired.raw()].into();
    p.set(Global::Story, 20_701_000);
    check(&p, &[])?;
    p.set(Global::Story, 20_701_001);
    check(&p, &[54])?;

    p = progress();
    p.set(Global::Story, 1_403_000);
    p.set(Global::PalmacostaRanchDorrRoute, 15_001);
    check(&p, &[])?;
    p.set(Global::PalmacostaRanchDorrRoute, 15_000);
    p.set(Global::BookOfRegeneration, 1000);
    check(&p, &[60])?;
    p.set(Global::BookOfRegeneration, 999);
    p.items = [(54, 1)].into();
    check(&p, &[57, 60])?;
    p.items = [(54, 1), (64, 1)].into();
    check(&p, &[60])?;
    p.items = [(54, 1), (64, 1), (52, 1)].into();
    p.set(Global::Hima, 300);
    check(&p, &[57, 60])?;
    p.set(Global::Hima, 301);
    check(&p, &[55, 60])?;
    p.set(Global::Hima, 302);
    check(&p, &[60])?;

    Ok(())
}

#[test]
fn returning_inside_a_wrapped_entrance_pushes_clear_without_changing_height() -> Result<()> {
    let mut data = definitions();
    data.worlds[0][0].position = [10., 3200.];
    data.worlds[0][0].interaction = Interaction::Active;
    data.worlds[0][0].radius = 80.;
    let locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    let inside = Position::from_map([76790., 3200., 200.])?;
    let outside = locations.push_out(World::Sylvarant, inside, Mount::Foot)?;
    assert!(
        locations
            .contact(World::Sylvarant, outside, Mount::Foot, 200.)
            .is_none()
    );
    assert!((outside.map()[0] - 76670.).abs() < 0.01);
    assert_eq!(outside.map()[2], 200.);
    assert_eq!(
        locations.push_out(World::Sylvarant, outside, Mount::Foot)?,
        outside
    );
    assert_eq!(
        locations.push_out(World::Sylvarant, inside, Mount::Rheairds)?,
        inside
    );
    Ok(())
}

#[test]
fn wrapped_contact_preserves_source_priority_octants_and_mount_exclusions() -> Result<()> {
    let mut data = definitions();
    for entry in &mut data.worlds[0][..2] {
        entry.position = [10., 3200.];
        entry.interaction = Interaction::Active;
    }
    let locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    assert_eq!(
        locations.contact(
            World::Sylvarant,
            Position::from_map([76790., 3200., 0.])?,
            Mount::Foot,
            0.
        ),
        Some(Contact {
            id: 1,
            direction: 4,
            blocked: false
        })
    );
    for (point, direction) in [
        ([30., 3200., 0.], 0),
        ([10., 3220., 0.], 2),
        ([10., 3180., 0.], 6),
    ] {
        assert_eq!(
            locations
                .contact(
                    World::Sylvarant,
                    Position::from_map(point)?,
                    Mount::Foot,
                    0.
                )
                .unwrap()
                .direction,
            direction
        );
    }
    assert!(
        locations
            .contact(
                World::Sylvarant,
                Position::from_map([10., 3200., 0.])?,
                Mount::Rheairds,
                300.
            )
            .is_none()
    );
    let mut data = definitions();
    data.worlds[0][62].marker = Marker::FieldPoint;
    let position = Position::from_map([data.worlds[0][62].position[0], 10000., 0.])?;
    let mut locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    locations.refresh(&progress().view())?;
    assert_eq!(
        locations
            .contact(World::Sylvarant, position, Mount::Foot, 0.)
            .unwrap()
            .id,
        63
    );
    assert!(
        locations
            .contact(World::Sylvarant, position, Mount::Noishe, 0.)
            .is_none()
    );
    assert!(
        locations
            .contact(World::Sylvarant, position, Mount::Ship, 0.)
            .is_none()
    );
    Ok(())
}

#[test]
fn discoveries_party_requirements_and_guidepost_unlocks_change_contacts() -> Result<()> {
    let mut data = definitions();
    data.worlds[0][44].interaction = Interaction::Active;
    data.worlds[0][44].guidepost = Some(Guidepost {
        name: "Region".into(),
        discovery_flag: std::num::NonZeroU16::new(901).unwrap(),
        unlock_flags: vec![],
    });
    let mut locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    let mut p = progress();
    locations.refresh(&p.view())?;
    assert_eq!(
        locations.appearance(63).unwrap().interaction,
        Interaction::Active
    );
    p.formation = vec![1];
    locations.refresh(&p.view())?;
    assert_eq!(
        locations.appearance(63).unwrap().interaction,
        Interaction::Disabled
    );
    p.formation = vec![1, 3];
    p.visited = [63].into();
    p.event_flags = [901].into();
    locations.refresh(&p.view())?;
    assert_eq!(
        locations.appearance(63).unwrap().interaction,
        Interaction::Disabled
    );
    assert_eq!(
        locations.appearance(45).unwrap().interaction,
        Interaction::Blocked
    );
    Ok(())
}

#[test]
fn story_boundaries_rebuild_visuals_and_air_contact_requires_dragon_altitude() -> Result<()> {
    let mut data = definitions();
    data.worlds[1][47].height = Some(1500.);
    let point = Position::from_map([data.worlds[1][47].position[0], 10000., 0.])?;
    let mut locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    let mut p = progress();
    p.set(Global::Story, 12203999);
    locations.refresh(&p.view())?;
    assert!(
        locations
            .contact(World::TetheAlla, point, Mount::Rheairds, 1500.)
            .is_none()
    );
    p.set(Global::Story, 12_204_000);
    locations.refresh(&p.view())?;
    assert!(
        locations
            .contact(World::TetheAlla, point, Mount::Rheairds, 1300.)
            .is_none()
    );
    assert_eq!(
        locations.contact(World::TetheAlla, point, Mount::Rheairds, 1301.),
        Some(Contact {
            id: 304,
            direction: 6,
            blocked: true
        })
    );
    p.set(Global::Story, 12400001);
    locations.refresh(&p.view())?;
    assert_eq!(locations.appearance(304).unwrap().marker, Marker::None);
    p.set(Global::Story, 20401000);
    locations.refresh(&p.view())?;
    assert_eq!(
        locations.appearance(24).unwrap(),
        Appearance {
            interaction: Interaction::Disabled,
            marker: Marker::None
        }
    );
    p.set(Global::Story, 0);
    locations.refresh(&p.view())?;
    assert_eq!(locations.appearance(24).unwrap().marker, Marker::Model(14));
    Ok(())
}

#[test]
fn reordered_locations_keep_ids_rewards_and_guideposts_and_change_contact_priority() -> Result<()> {
    let mut data = definitions();
    for entry in &mut data.worlds[0][..2] {
        entry.position = [1000., 2000.];
        entry.interaction = Interaction::Active;
        entry.marker = Marker::Model(11);
        entry.guidepost = Some(Guidepost {
            name: entry.name.clone(),
            discovery_flag: std::num::NonZeroU16::new(900 + entry.id).unwrap(),
            unlock_flags: vec![],
        });
    }
    data.worlds[0][1].reward_item = Some(42);
    let original = Locations::new(Arc::new(data.clone()), crate::overworld::scripts::fixture())?;
    let point = Position::from_map([1000., 2000., 0.])?;
    assert_eq!(
        original
            .contact(World::Sylvarant, point, Mount::Foot, 0.)
            .unwrap()
            .id,
        1
    );
    data.worlds[0].swap(0, 1);
    let data: Landmarks = serde_json::from_slice(&serde_json::to_vec(&data)?)?;
    let mut changed = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    assert_eq!(changed.definition(1).unwrap().name, "Place 1");
    assert_eq!(changed.definition(2).unwrap().name, "Place 2");
    assert_eq!(changed.reward(2), Some(42));
    assert_eq!(changed.reward(1), None);
    assert_eq!(
        changed
            .contact(World::Sylvarant, point, Mount::Foot, 0.)
            .unwrap()
            .id,
        2
    );
    assert_eq!(changed.guideposts, [2, 1]);
    assert_eq!(changed.guidepost(1).unwrap().discovery_flag.get(), 901);
    let mut progress = progress();
    progress.event_flags.insert(901);
    changed.refresh(&progress.view())?;
    assert_eq!(
        changed.appearance(1).unwrap().interaction,
        Interaction::Blocked
    );
    assert_eq!(
        original.appearance(1).unwrap().interaction,
        Interaction::Active
    );
    Ok(())
}

#[test]
#[ignore = "requires current landmarks and saved movement metadata; no terrain or media loading"]
fn installed_flight_floors_preserve_activation_seams_and_clearance_boundaries() -> Result<()> {
    use serde_json::Value;
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let evidence = repo.join("local/structure-review-20261027/world-movement");
    let library =
        repo.join("local/layout-migration/final-integration-20261025-supervised/library/world");
    let old: Value =
        serde_json::from_slice(&std::fs::read(evidence.join("before/world/movement.json"))?)?;
    let data: Landmarks = serde_json::from_slice(&std::fs::read(library.join("landmarks.json"))?)?;
    let movement: resonance_content::overworld::MovementParameters =
        serde_json::from_slice(&std::fs::read(library.join("movement.json"))?)?;
    movement.validate()?;
    let mut locations = Locations::new(Arc::new(data), crate::overworld::scripts::fixture())?;
    let owners = [23, 25, 266, 274];
    let mut cases = 0;
    for world in [World::Sylvarant, World::TetheAlla] {
        let zones = old["flight_zones"][world.index()].as_array().unwrap();
        let resolve = |zone: &Value| {
            let height = zone["height"].as_f64().unwrap() as f32;
            if let Some(id) = zone["location"].as_u64() {
                let landmark = locations.definition(id as u16).unwrap();
                (
                    landmark.position,
                    landmark.radius + zone["margin"].as_f64().unwrap() as f32,
                    height,
                )
            } else {
                (
                    serde_json::from_value::<[f32; 2]>(zone["position"].clone()).unwrap(),
                    zone["radius"].as_f64().unwrap() as f32,
                    height,
                )
            }
        };
        let geometry = zones.iter().map(resolve).collect::<Vec<_>>();
        for mask in 0..16 {
            for (index, id) in owners.into_iter().enumerate() {
                locations.appearances.get_mut(&id).unwrap().interaction =
                    if mask & (1 << index) == 0 {
                        Interaction::Active
                    } else {
                        Interaction::Disabled
                    };
            }
            for &(center, radius, _) in &geometry {
                for distance in [0., radius - 1., radius, radius + 1., radius * 2.] {
                    for heading in 0..8 {
                        let angle = heading as f32 * std::f32::consts::FRAC_PI_4;
                        for height in [0., 800., 1500.] {
                            let point = Position::from_map([
                                center[0] + distance * angle.cos(),
                                center[1] + distance * angle.sin(),
                                height,
                            ])?;
                            let mut expected = height;
                            for (zone, &(center, radius, floor)) in zones.iter().zip(&geometry) {
                                let disabled = zone["location"].as_u64().is_some_and(|id| {
                                    locations.appearances[&(id as u16)].interaction
                                        == Interaction::Disabled
                                });
                                if zone["active_only"] == true && disabled {
                                    continue;
                                }
                                if Position::from_map([center[0], center[1], 0.])?
                                    .distance_to(point)
                                    <= radius
                                {
                                    expected = expected.max(floor);
                                }
                            }
                            assert_eq!(
                                locations.flight_floor(
                                    world,
                                    point,
                                    movement.flight_zones.get(world)
                                )?,
                                expected,
                                "{world:?} mask {mask} point {point:?}"
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    std::fs::write(
        evidence.join("flight-comparison.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"cases":cases,"landmark_floors":4,"circular_zones":2,"activation_patterns":16}),
        )?,
    )?;
    Ok(())
}
