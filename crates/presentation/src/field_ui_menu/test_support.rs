use super::*;

pub(super) struct Fixture {
    pub(super) art: MenuArt,
    pub(super) font: BitmapFont,
    pub(super) dialogue: DialogueArt,
    pub(super) session: std::sync::Arc<resonance_content::session::SessionData>,
    pub(super) data: std::sync::Arc<resonance_content::menu_data::MenuData>,
    pub(super) party: resonance_events::party::Party,
}
impl Fixture {
    pub(super) fn load() -> Result<Self> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        );
        let read = |path: &str| -> Result<Vec<u8>> { Ok(std::fs::read(root.join(path))?) };
        let art: MenuArt = serde_json::from_slice(&read("ui/menu.json")?)?;
        let dialogue: DialogueArt = serde_json::from_slice(&read("ui/dialogue.json")?)?;
        let font = serde_json::from_slice(&read(&dialogue.font)?)?;
        let mut session_data: resonance_content::session::SessionData =
            serde_json::from_slice(&read("game/session-data.json")?)?;
        let data: std::sync::Arc<resonance_content::menu_data::MenuData> =
            std::sync::Arc::new(serde_json::from_slice(&read("game/menu-data.json")?)?);
        art.validate(data.items.len())?;
        session_data.rules = Some(data.clone());
        let session = std::sync::Arc::new(session_data);
        let mut party = resonance_events::party::Party::new(&session, Default::default())?;
        party.formation = vec![1, 2, 3, 4];
        party.members[0].name = Some("Aster".into());
        for (index, member) in party.members.iter_mut().take(4).enumerate() {
            member.techniques = session.characters[index]
                .allowed_techniques
                .iter()
                .copied()
                .collect();
            member.shortcuts =
                std::array::from_fn(|slot| session.characters[index].allowed_techniques[slot]);
            for &technique in &member.shortcuts {
                member.technique_uses.insert(technique, 17);
            }
        }
        Ok(Self {
            art,
            font,
            dialogue,
            session,
            data,
            party,
        })
    }
    pub(super) fn artwork(&self, world: &mut World) -> Result<MenuArtwork> {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let mut materials = Assets::<Surface>::default();
        let shared = materials.add(Surface {
            source: Handle::default(),
            sampling: Handle::default(),
            frame_mask: Handle::default(),
            color_mask: Handle::default(),
            coverage: Coverage::default(),
            additive: false,
            red_channel: false,
            opaque: false,
        });
        let artwork = MenuArtwork::load(
            |_| Ok(serde_json::to_vec(&self.art)?),
            self.session.experience.clone().into(),
            app.world().resource::<AssetServer>(),
            &mut materials,
            (&shared, [self.font.width, self.font.height]),
            &resonance_content::diagnostics::Diagnostics::new(true),
        )?;
        world.insert_resource(app.world_mut().remove_resource::<Assets<Image>>().unwrap());
        world.insert_resource(materials);
        Ok(artwork)
    }
    pub(super) fn drawing(&self, tick: u32) -> Drawing<'_> {
        Drawing {
            screen: [0., 0., 640., 448.],
            spec: &self.art,
            font: &self.font,
            selection: &self.dialogue.selection,
            preferences: Some(&self.party.settings.preferences),
            experience: &self.session.experience,
            tick,
            plane: 1,
            opacity: 255,
            offset: [0.; 2],
            batches: BTreeMap::new(),
        }
    }
    pub(super) fn field_menu(&self) -> Menu {
        let mut menu = Menu::new(
            resonance_game::menu::Page::Main,
            Some(resonance_game::Checkpoint::Field(
                resonance_game::field::FieldCheckpoint {
                    map_id: 330,
                    position: [0.; 3],
                    heading: 0.,
                    camera: {
                        let mut rig = resonance_events::camera::CameraRig::default();
                        *rig.current_mut() =
                            resonance_events::camera::EntryCamera::following(1).camera;
                        Some(rig.settings(1).unwrap())
                    },
                    played_ticks: 17,
                    allow_incomplete_scripts: false,
                    progress: resonance_events::SavedProgress {
                        script_globals: vec![0; 256],
                        script_state: Default::default(),
                        party: self.party.clone(),
                        event_flags: Default::default(),
                        event_records: Default::default(),
                        random_state: 0,
                        gameplay_random: Default::default(),
                        tick: 17,
                    },
                },
            )),
            false,
        );
        menu.main_fade = 0;
        menu.resources = Some(std::sync::Arc::new(resonance_game::menu::Resources {
            session: self.session.clone(),
            data: self.data.clone(),
            files: Default::default(),
        }));
        menu
    }
}
