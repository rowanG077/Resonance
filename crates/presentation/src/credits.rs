//! Prepared credit pages scroll against the music clock while the field waits.
use super::*;
use crate::audio_output::{Player, Sink};
use resonance_content::credits::{Manifest, Operation};
use resonance_events::session_screen::{Request, Target};
use resonance_game::clock::UPDATE_HZ;
use resonance_playback::{ChannelCount, Decodable, SOURCE_RATE, SampleRate, Source};
use std::sync::Arc;

const UI_DEPTH: f32 = 500.;

#[cfg(test)]
#[path = "credits_tests.rs"]
mod tests;

struct Draw {
    batch: Batch,
    size: [u32; 2],
    material: Handle<Surface>,
}

pub(super) struct Artwork {
    manifest: Manifest,
    images: Vec<Handle<Image>>,
    draws: Vec<Draw>,
    audio: Audio,
    pub layers: Vec<Layer>,
}

impl Artwork {
    pub fn load(
        files: &resonance_content::prepared::Files,
        font: &BitmapFont,
        font_material: &Handle<Surface>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
    ) -> Result<Self> {
        let manifest: Manifest = files.json(resonance_content::credits::PATH)?;
        manifest.validate()?;
        let audio = Audio {
            clip: Arc::new(crate::field_audio::Clip::prepare(
                files.read(&manifest.music.asset.path)?,
                &manifest.music,
            )?),
            rate: manifest.music.sample_rate,
            gain: 1.,
            mono: false,
        };
        let batches = layout(&manifest, font)?;
        let mut images = Vec::new();
        let mut surfaces = Vec::new();
        for picture in &manifest.pictures {
            let image = server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| {
                    s.is_srgb = false;
                })
                .load(picture.path.clone());
            surfaces.push(materials.add(Surface {
                source: image.clone(),
                sampling: image.clone(),
                frame_mask: image.clone(),
                color_mask: image.clone(),
                coverage: default(),
                opaque: false,
                additive: false,
                red_channel: false,
            }));
            images.push(image);
        }
        let mut background = materials
            .get(font_material)
            .context("credits font is missing")?
            .clone();
        background.opaque = true;
        let background = materials.add(background);
        let draws = batches
            .into_iter()
            .enumerate()
            .map(|(i, batch)| Draw {
                batch,
                size: if i < 2 {
                    [font.width, font.height]
                } else {
                    let picture = &manifest.pictures[i - 2];
                    [picture.width, picture.height]
                },
                material: match i {
                    0 => background.clone(),
                    1 => font_material.clone(),
                    _ => surfaces[i - 2].clone(),
                },
            })
            .filter(|draw| !draw.batch.indices.is_empty())
            .collect();
        Ok(Self {
            manifest,
            images,
            draws,
            audio,
            layers: Vec::new(),
        })
    }

    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.images.iter().all(|image| images.contains(image.id()))
    }

    pub fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if !self.layers.is_empty() {
            return;
        }
        for (i, draw) in self.draws.iter().enumerate() {
            let mesh = meshes.add(draw.batch.clone().mesh(draw.size));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(draw.material.clone()),
                    Transform::from_xyz(0., 0., UI_DEPTH + i as f32),
                    Visibility::Hidden,
                ))
                .id();
            self.layers.push(Layer {
                entity,
                mesh,
                material: draw.material.clone(),
                uploaded: None,
                visible: false,
            });
        }
    }

    pub fn render(&mut self, request: Option<&Request>, commands: &mut Commands) {
        let progress = request
            .filter(|r| r.target == Target::Credits)
            .map(|r| r.operation.progress());
        let scroll = &self.manifest.program.scroll;
        let offset = progress.map_or(0., |p| {
            (p.position as f64 * f64::from(scroll.height_pixels)
                / f64::from(scroll.speed_divisor_ticks))
            .min(f64::from(
                scroll.height_pixels - i32::from(scroll.stop_margin_pixels),
            )) as f32
        });
        for (i, layer) in self.layers.iter_mut().enumerate() {
            layer.show(progress.is_some_and(|p| p.ready), commands);
            commands.entity(layer.entity).insert(Transform::from_xyz(
                0.,
                if i == 0 { 0. } else { offset },
                UI_DEPTH + i as f32,
            ));
        }
    }
}

/// Compose each texture batch once; scrolling only changes their transforms.
fn layout(manifest: &Manifest, font: &BitmapFont) -> Result<Vec<Batch>> {
    let program = &manifest.program;
    let [width, height] = program.style.glyph_size.map(f32::from);
    let advance = |c: char| -> Result<f32> {
        Ok(font
            .glyphs
            .get(&c)
            .with_context(|| format!("uncooked credits glyph {c:?}"))?
            .advance as f32
            * width
            / height)
    };
    let mut batches = vec![Batch::default(); 2 + manifest.pictures.len()];
    batches[0].quad(
        [
            0.,
            0.,
            f32::from(program.canvas[0]),
            f32::from(program.canvas[1]),
        ],
        [0.5; 4],
        [0., 0., 0., 1.],
    );
    let (mut x, mut y) = (0., 0.);
    for (i, op) in program.operations.iter().enumerate() {
        match op {
            Operation::Text { text } => {
                for c in text.chars() {
                    let glyph = font
                        .glyphs
                        .get(&c)
                        .with_context(|| format!("uncooked credits glyph {c:?}"))?;
                    let [u, v, w, h] = glyph.rect.map(|v| v as f32);
                    batches[1].quad(
                        [x, y, x + width, y + height],
                        [u, v, u + w, v + h],
                        program.style.color.map(|v| f32::from(v) / 255.),
                    );
                    x += advance(c)?;
                }
            }
            Operation::CenterLine => {
                let mut length = 0.;
                for op in program.operations[i + 1..].iter().take_while(|op| {
                    !matches!(op, Operation::Newline | Operation::VerticalSpace { .. })
                }) {
                    match op {
                        Operation::Text { text } => {
                            for c in text.chars() {
                                length += advance(c)?;
                            }
                        }
                        Operation::Tab => length += f32::from(program.style.tab_width),
                        _ => {}
                    }
                }
                x = (f32::from(program.canvas[0]) - length) / 2.;
            }
            Operation::Newline => {
                x = 0.;
                y += f32::from(program.style.line_height);
            }
            Operation::Tab => x += f32::from(program.style.tab_width),
            Operation::VerticalSpace { pixels } => {
                x = 0.;
                y += *pixels as f32;
            }
            Operation::Picture { index } => {
                let picture = &manifest.pictures[usize::from(*index)];
                let (w, h) = (picture.width as f32, picture.height as f32);
                batches[2 + usize::from(*index)].quad(
                    [x, y, x + w, y + h],
                    [0., 0., w, h],
                    [1.; 4],
                );
            }
            Operation::IgnoredControl { .. } => {}
        }
    }
    Ok(batches)
}

#[derive(Asset, TypePath, Clone)]
pub(crate) struct Audio {
    clip: Arc<crate::field_audio::Clip>,
    rate: u32,
    gain: f32,
    mono: bool,
}

pub(crate) struct Frames {
    audio: Audio,
    frame: u64,
    right: Option<f32>,
}
impl Iterator for Frames {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        let mut sample = self.audio.clip.sample(
            self.frame * u64::from(self.audio.rate),
            SOURCE_RATE,
            [self.audio.gain; 2],
        )?;
        self.frame += 1;
        if self.audio.mono {
            let mono = (sample[0] + sample[1]) / 2.;
            sample.fill(mono);
        }
        self.right = Some(sample[1]);
        Some(sample[0])
    }
}
impl Source for Frames {
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SOURCE_RATE).unwrap()
    }
}
impl Decodable for Audio {
    type Decoder = Frames;
    fn decoder(&self) -> Frames {
        Frames {
            audio: self.clone(),
            frame: 0,
            right: None,
        }
    }
}

#[derive(Resource)]
pub(crate) struct Playback {
    operation: resonance_events::Operation,
    audio: Entity,
    ticks: u32,
    tail: Option<u32>,
    final_hold: u32,
}
impl Playback {
    pub fn brightness(&self) -> f32 {
        self.tail
            .map_or(1., |age| 1. - age as f32 / self.final_hold.max(1) as f32)
            .clamp(0., 1.)
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_asset::<Audio>().add_systems(
        FixedUpdate,
        advance
            .run_if(crate::dungeons::running)
            .after(crate::timing::advance_clock)
            .before(crate::field_view::advance_live),
    );
}

pub(crate) fn start(world: &mut World) -> Result<()> {
    if world.contains_resource::<Playback>() {
        return Ok(());
    }
    let art = world
        .resource::<super::Artwork>()
        .credits
        .as_ref()
        .context("credits were not prepared")?;
    let audio = art.audio.clone();
    let final_hold = art.manifest.final_hold_ticks;
    begin(world, audio, final_hold)
}

fn begin(world: &mut World, mut audio: Audio, final_hold: u32) -> Result<()> {
    let events = world.resource::<crate::new_game::Session>().events();
    let operation = events
        .world
        .screen_request
        .as_ref()
        .context("missing credits request")?
        .operation
        .clone();
    if let Some(party) = &events.world.party {
        audio.gain = f32::from(party.settings.preferences.volumes.music) / 127.;
        audio.mono = !party.settings.preferences.stereo;
    }
    crate::field_audio::leave_field(world)?;
    let audio = world.resource_mut::<Assets<Audio>>().add(audio);
    let audio = world.spawn(Player(audio)).id();
    operation.advance(0).map_err(anyhow::Error::msg)?;
    world.insert_resource(Playback {
        operation,
        audio,
        ticks: 0,
        tail: None,
        final_hold,
    });
    Ok(())
}

pub(crate) fn retire(world: &mut World) {
    if let Some(playback) = world.remove_resource::<Playback>() {
        world.despawn(playback.audio);
    }
}

pub(crate) fn retire_cancelled(world: &mut World) {
    if world
        .get_resource::<Playback>()
        .is_some_and(|p| !p.operation.is_pending())
    {
        retire(world);
    }
}

fn advance(world: &mut World) {
    let Some(playback) = world.get_resource::<Playback>() else {
        return;
    };
    if !playback.operation.is_pending() {
        retire(world);
        return;
    }
    let Some(sink) = world.get::<Sink>(playback.audio) else {
        return;
    };
    let (position, ended) = (sink.position(), sink.empty());
    let mut playback = world.resource_mut::<Playback>();
    if ended {
        playback.tail = Some(playback.tail.map_or(0, |age| age + 1));
    }
    let ticks = (position.as_secs_f64() * UPDATE_HZ) as u32 + playback.tail.unwrap_or(0);
    playback.ticks = playback.ticks.max(ticks);
    let complete = playback.tail.is_some_and(|age| age >= playback.final_hold);
    if let Err(error) = playback.operation.advance(playback.ticks) {
        error!("Credits clock failed: {error}");
        world.write_message(AppExit::error());
        return;
    }
    let mut session = world.resource_mut::<crate::new_game::Session>();
    session.advance_play_time();
    session.events_mut().world.played_ticks = session.play_time().total();
    if complete {
        let request = session.events_mut().world.screen_request.take().unwrap();
        if let Err(error) = request.operation.complete(Some(0)) {
            error!("Credits completion failed: {error}");
            world.write_message(AppExit::error());
        }
        retire(world);
    }
}
