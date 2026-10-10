use super::*;
use resonance_content::{
    diagnostics::Diagnostics,
    session::{CharacterDefinition, ItemDefinition, SessionData, StatGrowth},
    skit::{SkitCatalog, SkitCondition, SkitDefinition, SkitLocation, SkitResourcePaths},
};

fn session(paranoid: bool) -> FieldSession {
    session_with_resources(paranoid, ResourceLibrary::default())
}

fn session_with_resources(paranoid: bool, resources: ResourceLibrary) -> FieldSession {
    let mut field = super::tests::choice_session();
    // A finished entry and an ordinary interaction on actor two.
    let program = [10u16, 0, 0, 1, 0, 0, 0, 2, 0, 1, 0x20ff, 0x20ff];
    field.events = EventRuntime::new(
        Arc::new(
            Program::decode(
                &program
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ),
        Arc::new(resources),
    )
    .unwrap();
    field.set_diagnostics(Diagnostics::new(paranoid));
    field.events.world.input_enabled = true;
    field.events.world.controlled_actor = 1;
    // Keep ordinary movement's floor-clearance probes inside this test field.
    field.walkmesh = navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
        surface: 0,
        vertices: vec![
            [-100., -100., 0.],
            [100., -100., 0.],
            [100., 100., 0.],
            [-100., 100., 0.],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }])
    .unwrap();
    let mut player = Actor::new(1, [0.; 3]);
    player.face(90.);
    field.events.world.insert_actor(1, player);
    field
}

fn session_data() -> SessionData {
    SessionData {
        rules: None,
        version: 1,
        executable_sha256: "0".repeat(64),
        items: vec![ItemDefinition {
            equipment_kind: None,
            allowed_characters: 0,
            stack_limit: 20,
        }],
        experience: vec![0, 0, 100],
        characters: vec![
            CharacterDefinition {
                cooking: [0; resonance_content::menu_data::RECIPE_COUNT],
                ex_skills: [0; 4],
                ex_gems: [0; 4],
                compound_ex_skills: vec![],
                recent_compound_ex_skills: vec![],
                technique_balance: 0,
                affinity: 0,
                level: 1,
                experience: 0,
                base_stats: [100, 10, 10, 10, 10, 10, 10],
                luck: 0,
                overlimit: 0,
                equipment: [0; 6],
                techniques: vec![],

                allowed_techniques: vec![],
                shortcuts: [0; 4],
                growth: std::array::from_fn(|_| StatGrowth {
                    base: 0,
                    random: 0,
                    title_bonus: 0
                }),
                level_techniques: BTreeMap::new(),
            };
            9
        ],
    }
}

fn catalog(first: SkitCondition) -> Arc<SkitCatalog> {
    Arc::new(SkitCatalog {
        version: 2,
        preview_order: Vec::new(),
        skits: [first, SkitCondition::None]
            .into_iter()
            .enumerate()
            .map(|(index, condition)| SkitDefinition {
                id: index as u16 + 1,
                title: format!("Skit {index}"),
                story: None,
                party_mask: 0,
                location: SkitLocation::Anywhere,
                condition,
            })
            .collect(),
        resources: (1..=2)
            .map(|id| {
                (
                    id,
                    SkitResourcePaths {
                        title: None,
                        script: format!("skits/{id}.script"),
                        messages: format!("skits/{id}.json"),
                    },
                )
            })
            .collect(),
        portraits: BTreeMap::new(),
        portrait_recipes: vec![],
        media: BTreeMap::new(),
    })
}

#[test]
fn unsupported_hint_omits_only_the_notice_unless_paranoid() {
    for paranoid in [false, true] {
        let mut field = session(paranoid);
        let mut actor = Actor::new(2, [60., 0., 0.]);
        actor.collidable = false;
        actor.interaction_label = 99;
        field.events.world.insert_actor(2, actor);
        assert_eq!(field.interaction_target(), Some(2));
        let movement = FieldInput {
            direction: [1., 0.],
            ..Default::default()
        };
        let update = field.step(movement);
        assert_eq!(update.is_err(), paranoid);
        assert!(field.diagnostics.has_errors());
        if !paranoid {
            assert!(field.action_prompt().is_none());
            assert!(field.player_has_control());
            let position = field.events.world.actors[&1].position[0];
            assert!(position > 0.);
            field.step(movement).unwrap();
            assert!(field.events.world.actors[&1].position[0] > position);
            assert!(field.player_has_control());
            assert_eq!(field.diagnostics.entries().len(), 1);
        }
    }
}

#[test]
fn missing_optional_skit_is_not_advertised_or_carried_into_the_next_field() {
    for paranoid in [false, true] {
        let data = Arc::new(session_data());
        let text = Arc::new(resonance_content::session::GameText::default());
        let mut field = session_with_resources(
            paranoid,
            ResourceLibrary {
                session_data: Some(data.clone()),
                text: text.clone(),
                ..Default::default()
            },
        );
        field.events.world.party =
            Some(resonance_events::party::Party::new(&data, Default::default()).unwrap());
        field.skits = skit::Skits::new(Some(catalog(SkitCondition::None)));
        let mut files = resonance_content::prepared::Files::default();
        files.insert(
            "skits/2.script".into(),
            Arc::from([0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]),
        );
        files.insert("skits/2.json".into(), Arc::from(b"[]".as_slice()));
        assert_eq!(field.prepare_skits(&files).is_err(), paranoid);
        assert!(field.diagnostics.has_errors());
        if !paranoid {
            assert_eq!(field.skit_programs.keys().copied().collect::<Vec<_>>(), [2]);
            let previous = session(false);
            field.continue_ambient(&previous);
            field.step(Default::default()).unwrap();
            assert_eq!(field.skit_prompt().unwrap().id, 2);
            assert!(field.player_has_control());
            field.start_skit(2, true, false, None).unwrap();
            let resources = field.active_skit.as_ref().unwrap().events.resources();
            assert!(Arc::ptr_eq(resources.session_data.as_ref().unwrap(), &data));
            assert!(Arc::ptr_eq(&resources.text, &text));
        }
    }
}
