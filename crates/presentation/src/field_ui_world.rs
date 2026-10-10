//! World prompts and skits share the field bitmap font and portrait renderer.
use super::*;
#[path = "field_ui_world_map.rs"]
mod map;

pub(crate) struct Artwork {
    common: super::Artwork,
    layer: Layer,
    map: map::Artwork,
    camera_mode: Option<bool>,
    camera_notice: Option<u32>,
}
impl Artwork {
    pub fn load(
        files: &resonance_content::prepared::Files,
        server: &AssetServer,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
        images: &mut Assets<Image>,
    ) -> Result<Self> {
        let mut common = super::Artwork::load_shared(
            files,
            files
                .json::<resonance_content::session::SessionData>("game/session-data.json")?
                .experience
                .into(),
            &BTreeMap::new(),
            server,
            materials,
            images,
        )?;
        common.prepare(commands, meshes, materials);
        let map = map::Artwork::prepare(&common.menu, commands, meshes);
        let mut placeholder = Batch::default();
        placeholder.quad([0., 0., 1., 1.], [0.5; 4], [0.; 4]);
        let mesh = meshes.add(placeholder.mesh([common.font.width, common.font.height]));
        let material = common.surfaces[9].clone();
        let entity = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., 3.),
                Visibility::Hidden,
            ))
            .id();
        let layer = Layer {
            entity,
            mesh,
            material,
            uploaded: None,
            visible: false,
        };
        Ok(Self {
            common,
            layer,
            map,
            camera_mode: None,
            camera_notice: None,
        })
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.common.ready(images)
    }
    pub fn despawn(mut self, world: &mut World) {
        self.map.despawn(world);
        world.despawn(self.layer.entity);
        self.common.despawn(world);
    }
    #[allow(clippy::too_many_arguments)] // Shared UI artwork and Bevy asset stores.
    pub fn render(
        &mut self,
        session: &resonance_game::overworld::Session,
        text: &resonance_content::session::GameText,
        resolution: crate::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<Surface>,
    ) -> Result<()> {
        let mut batch = Batch::default();
        let camera_mode = session.travel.state().alternate_perspective;
        if self
            .camera_mode
            .is_some_and(|previous| previous != camera_mode)
        {
            self.camera_notice = Some(session.events.tick());
        }
        self.camera_mode = Some(camera_mode);
        if session.player_has_control()
            && self
                .camera_notice
                .is_some_and(|start| session.events.tick().saturating_sub(start) < 120)
        {
            skit::centered(
                &mut batch,
                &self.common.font,
                if camera_mode {
                    "Camera: distant view"
                } else {
                    "Camera: close view"
                },
                408.,
                18.,
                1.,
                false,
            )?;
        }
        self.map.render(session, &mut batch, commands, meshes)?;
        if let Some((line, alpha)) = session.cinematic.as_ref().and_then(|c| c.dialogue()) {
            let lines: Vec<_> = line.text.lines().collect();
            let y = 424. - lines.len() as f32 * 26.;
            batch.quad(
                [0., y - 16., 640., 440.],
                [0.5; 4],
                [0., 0., 0., alpha * 0.5],
            );
            let width = lines
                .iter()
                .map(|line| cinematic_width(&self.common.font, line, 24.))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .fold(0., f32::max);
            let x = ((640. - width) / 2.).trunc();
            for (index, line) in lines.into_iter().enumerate() {
                cinematic_text(
                    &mut batch,
                    &self.common.font,
                    line,
                    [x, y + 8. + index as f32 * 26.],
                    24.,
                    alpha,
                )?;
            }
            let name = session
                .events
                .world
                .party
                .as_ref()
                .and_then(|party| party.members.get(usize::from(line.speaker) - 1))
                .and_then(|member| member.name.as_ref())
                .or_else(|| text.characters.get(&i32::from(line.speaker)));
            if let Some(name) = name {
                let width = cinematic_width(&self.common.font, name, 16.)?;
                cinematic_text(
                    &mut batch,
                    &self.common.font,
                    name,
                    [x - width, y - 8.],
                    16.,
                    alpha,
                )?;
            }
        }
        let prompt = match session.prompt() {
            Some(resonance_game::overworld::Prompt::ChangeWorld { destination }) => Some(
                if *destination == resonance_game::overworld::World::Sylvarant {
                    "Go to Sylvarant?"
                } else {
                    "Go to Tethe'alla?"
                }
                .into(),
            ),
            Some(resonance_game::overworld::Prompt::Enter { name, .. }) => Some(name.clone()),
            Some(resonance_game::overworld::Prompt::Guidepost { name }) => {
                Some(format!("Long-range Mode is now available in\n{name}."))
            }
            Some(resonance_game::overworld::Prompt::Item { item, received }) => {
                Some(if *received {
                    format!(
                        "Obtained {}.",
                        text.items.get(item).context("world item name missing")?
                    )
                } else {
                    format!(
                        "Found {},\nbut you cannot carry any more.",
                        text.items.get(item).context("world item name missing")?
                    )
                })
            }
            None => None,
        };
        self.common
            .render_world_notifications(session, commands, meshes)?;
        if !batch.indices.is_empty() {
            self.layer.update_mesh(
                batch.clone(),
                [self.common.font.width, self.common.font.height],
                meshes,
            )?;
        }
        self.layer.show(!batch.indices.is_empty(), commands);
        self.common.skits.render(
            session.active_skit.as_ref(),
            &self.common.font,
            resolution,
            commands,
            meshes,
            images,
        )?;
        self.common.resolution = resolution;
        let empty_dialogue = BTreeMap::new();
        let (world, dialogue) = session
            .active_skit
            .as_ref()
            .map_or((&session.events.world, &empty_dialogue), |skit| {
                (&skit.events.world, &skit.dialogue)
            });
        self.common.render_dialogue(
            world,
            dialogue,
            world,
            session
                .active_skit
                .as_ref()
                .map_or(session.events.tick(), |skit| skit.events.tick()),
            &BTreeMap::new(),
            commands,
            meshes,
            materials,
        )?;
        self.common.menu.render(
            menu::Source::World(session, prompt.as_deref()),
            &self.common.font,
            &self.common.spec,
            session
                .menu
                .as_ref()
                .map_or(session.events.tick(), |m| m.tick),
            resolution,
            commands,
            meshes,
        )
    }
}

fn cinematic_width(font: &BitmapFont, text: &str, height: f32) -> Result<f32> {
    let characters: Vec<_> = text.chars().collect();
    characters
        .iter()
        .enumerate()
        .map(|(index, &c)| {
            let narrow = resonance_content::font::is_single_byte(c);
            let measure = if narrow {
                characters.get(index + 1).copied().unwrap_or(' ')
            } else {
                c
            };
            let glyph = font
                .glyphs
                .get(&measure)
                .context("missing cinematic glyph")?;
            Ok((glyph.advance as f32 * height / if narrow { 34. } else { 25. }).trunc())
        })
        .sum()
}
fn cinematic_text(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [mut x, y]: [f32; 2],
    height: f32,
    alpha: f32,
) -> Result<()> {
    for c in text.chars() {
        let glyph = font.glyphs.get(&c).context("missing cinematic glyph")?;
        let narrow = resonance_content::font::is_single_byte(c);
        let width = height * if narrow { 0.5 } else { 1. };
        batch.quad(
            [x, y, x + width, y + height],
            glyph_uv(glyph.rect),
            [1., 1., 1., alpha],
        );
        x += (glyph.advance as f32 * height / if narrow { 34. } else { 25. }).trunc();
    }
    Ok(())
}
