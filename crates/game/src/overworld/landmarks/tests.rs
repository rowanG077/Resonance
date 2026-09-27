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
                    name: format!("Place {id}"),
                    automatic: false,
                })
                .collect()
        }),
        item_rewards: Default::default(),
        party_requirements: [(63, 3)].into(),
    }
}
#[repr(u16)]
pub(crate) enum Global {
    Story = 0,
    Hima = 3,
    UnknownId30 = 30,
    PalmacostaRanch = 31,
    Asgard = 41,
    LinkiteTree = 60,
}

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
            .write(
                0x40 + global as u16 * 4,
                symphonia_script::Width::S32,
                value,
            )
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
fn caravan_camps_follow_story_quests_and_key_items_for_drawing_and_contact() -> Result<()> {
    let camps = [53, 54, 55, 57, 58, 60];
    let mut data = definitions();
    for id in camps {
        // The catalogue exposes every camp; story rules select the active stops.
        data.worlds[0][id - 1].interaction = Interaction::Active;
        data.worlds[0][id - 1].marker = Marker::Model { id: 11 };
    }
    let mut locations = Locations::new(
        Arc::new(data),
        Default::default(),
        crate::overworld::scripts::fixture(),
    )?;
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
    p.event_flags = [1287].into();
    p.set(Global::Story, 20_701_000);
    check(&p, &[])?;
    p.set(Global::Story, 20_701_001);
    check(&p, &[54])?;

    p = progress();
    p.set(Global::Story, 1_403_000);
    p.set(Global::PalmacostaRanch, 15_001);
    check(&p, &[])?;
    p.set(Global::PalmacostaRanch, 15_000);
    p.set(Global::Asgard, 1000);
    check(&p, &[60])?;
    p.set(Global::Asgard, 999);
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
    let locations = Locations::new(
        Arc::new(data),
        Default::default(),
        crate::overworld::scripts::fixture(),
    )?;
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
    let locations = Locations::new(
        Arc::new(data),
        Default::default(),
        crate::overworld::scripts::fixture(),
    )?;
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
    let mut locations = Locations::new(
        Arc::new(data),
        Default::default(),
        crate::overworld::scripts::fixture(),
    )?;
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
    let posts = vec![Guidepost {
        name: "Region".into(),
        name_id: 0,
        location: 45,
        event_flags: [std::num::NonZeroU16::new(901), None, None],
    }];
    let mut locations = Locations::new(
        Arc::new(data),
        Arc::new(posts),
        crate::overworld::scripts::fixture(),
    )?;
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
    let mut locations = Locations::new(
        Arc::new(data),
        Default::default(),
        crate::overworld::scripts::fixture(),
    )?;
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
    assert_eq!(
        locations.appearance(24).unwrap().marker,
        Marker::Model { id: 14 }
    );
    Ok(())
}
