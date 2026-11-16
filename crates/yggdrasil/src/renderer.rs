use crate::drawing::BlendMode;
use crate::{
    Error, HEIGHT, WIDTH,
    actors::Performers,
    data::{Assets, clamp, round},
    drawing::Drawing,
    exposure,
    lighting::{LightState, Lighting},
    timeline::{Event, SceneState, Timeline},
    tree::Tree,
};
use bevy::math::Vec4;

/// Rendering controls independent of a window, GPU, and importer.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub lighting: Lighting,
    pub motes: bool,
    pub foreground: bool,
    /// Animate motes and firelight; character motion follows the story clock.
    pub ambient_motion: bool,
    pub characters: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            lighting: Lighting::Auto,
            motes: true,
            foreground: true,
            ambient_motion: true,
            characters: true,
        }
    }
}

/// A sampled scene for Bevy to render. Artwork and drawing commands remain
/// separate, so the engine renders the actual parts rather than movie frames.
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) image: crate::drawing::Image,
    pub progress: f64,
    pub seconds: f64,
}

#[cfg(test)]
impl Frame {
    pub fn width(&self) -> u32 {
        self.image.width() as u32
    }
    pub fn height(&self) -> u32 {
        self.image.height() as u32
    }
}

/// Samples authored assets and choreography into native Bevy draw plans. Reuse one renderer
/// across exposures; the growing tree is cached at 601 progress checkpoints.
#[derive(bevy::prelude::Resource)]
pub struct Renderer {
    pub(crate) assets: Assets,
    pub(crate) tree: Tree,
    pub(crate) timeline: Timeline,
}
impl Renderer {
    pub fn new() -> Result<Self, Error> {
        let assets = Assets::load()?;
        let tree = Tree::new(&assets);
        let timeline = Timeline::new(&assets.data)?;
        Ok(Self {
            assets,
            tree,
            timeline,
        })
    }
    pub fn duration(&self) -> f64 {
        self.timeline.duration
    }
    pub fn events(&self) -> Vec<Event> {
        self.timeline.events()
    }
    /// Sample any playback mode. Tree growth can be driven independently by the host.
    pub(crate) fn render(
        &mut self,
        playback: &crate::Playback,
        seconds: f64,
        progress: Option<f64>,
        options: &Options,
    ) -> Result<Frame, Error> {
        let seconds = exposure(seconds);
        let (growth, state, event_time, auto_light) = match playback {
            crate::Playback::Journey => {
                let moment = self.timeline.at(seconds, &self.assets.data);
                (
                    moment.progress,
                    Some(moment.state),
                    moment.event_time,
                    moment.light,
                )
            }
            crate::Playback::Event(id) => {
                let visit = self
                    .timeline
                    .event(id)
                    .ok_or_else(|| Error::UnknownEvent(id.clone()))?;
                let time = visit.event.start + seconds;
                let state =
                    self.timeline
                        .with_world(visit.sample(seconds), time, &self.assets.data);
                (
                    visit.event.growth,
                    Some(state),
                    seconds,
                    self.timeline.light(time, &self.assets.data),
                )
            }
            crate::Playback::Growth => (
                0.,
                None,
                0.,
                LightState::at(progress.unwrap_or(0.), options.lighting),
            ),
        };
        let progress = progress.map(clamp).unwrap_or(growth);
        let light = if options.lighting == Lighting::Auto {
            auto_light
        } else {
            LightState::at(progress, options.lighting)
        };
        Ok(self.render_scene(
            progress,
            seconds,
            light,
            state.as_ref(),
            event_time,
            options,
        ))
    }
    fn render_scene(
        &mut self,
        progress: f64,
        seconds: f64,
        light: LightState,
        state: Option<&SceneState>,
        event_time: f64,
        options: &Options,
    ) -> Frame {
        self.tree.render(&self.assets, progress);
        let mut out = Drawing::new(WIDTH as i32, HEIGHT as i32);
        out.image_scaled(
            &self.assets.image(&self.assets.data.meadow),
            0.,
            0.,
            crate::WIDTH as f64,
            crate::HEIGHT as f64,
        );
        light.background(&mut out, &self.assets);
        out.ellipse(
            512.,
            505.,
            15. + 180. * progress,
            4. + 11. * progress,
            "#27422c",
            0.19,
        );
        let performers = Performers {
            assets: &self.assets,
            timeline: &self.timeline,
            tree: &self.tree,
            progress: round(progress * crate::tree::GROWTH_STEPS as f64)
                / crate::tree::GROWTH_STEPS as f64,
        };
        if light.night > 0. || light.twilight > 0. {
            let mut foreground = Drawing::new(crate::WIDTH as i32, crate::HEIGHT as i32);
            foreground.image_scaled(
                &self.tree.image,
                0.,
                0.,
                crate::WIDTH as f64,
                crate::HEIGHT as f64,
            );
            if options.characters
                && let Some(state) = state
            {
                performers.draw(&mut foreground, state, event_time, seconds);
            }
            let fire = state.and_then(|s| {
                s.props.iter().find(|p| {
                    p.kind == crate::scene::PropKind::Campfire
                        && p.visible
                        && p.attach.is_none()
                        && p.heat > 0.
                })
            });
            let mut warmth = Drawing::new(crate::WIDTH as i32, crate::HEIGHT as i32);
            if let Some(fire) = fire {
                let flicker =
                    0.975 + 0.025 * (if options.ambient_motion { seconds } else { 0. } * 2.4).sin();
                let strength = (0.38 + 0.28 * fire.heat) * light.night * flicker;
                warmth.image(&foreground.snapshot(), 0., 0.);
                warmth.fill(crate::SCENE_BOUNDS, "#ffb352", 0.2, BlendMode::SrcATop);
                warmth.radial(
                    [fire.x, fire.y - 25.],
                    8.,
                    185.,
                    &[
                        (0., Vec4::new(1., 1., 1., strength as f32)),
                        (0.55, Vec4::new(1., 1., 1., (strength * 0.55) as f32)),
                        (1., Vec4::new(1., 1., 1., 0.)),
                    ],
                    crate::SCENE_BOUNDS,
                    BlendMode::DstIn,
                );
                out.radial(
                    [fire.x, fire.y],
                    0.,
                    125.,
                    &[
                        (
                            0.,
                            Vec4::new(
                                238. / 255.,
                                154. / 255.,
                                65. / 255.,
                                (strength * 0.2) as f32,
                            ),
                        ),
                        (1., Vec4::new(238. / 255., 154. / 255., 65. / 255., 0.)),
                    ],
                    [fire.x - 125., fire.y - 125., 250., 250.],
                    BlendMode::SrcOver,
                );
            }
            light.tint(&mut foreground, 0.56);
            out.image(&foreground.snapshot(), 0., 0.);
            if fire.is_some() {
                out.image(&warmth.snapshot(), 0., 0.);
            }
            if options.characters
                && let Some(state) = state
            {
                for prop in state.props.iter().filter(|p| p.emissive) {
                    performers.prop(&mut out, prop, state.time, state.time);
                }
            }
        } else {
            out.image_scaled(
                &self.tree.image,
                0.,
                0.,
                crate::WIDTH as f64,
                crate::HEIGHT as f64,
            );
            if options.characters
                && let Some(state) = state
            {
                performers.draw(&mut out, state, event_time, seconds);
            }
        }
        self.finish(&mut out, progress, seconds, light, options);
        Frame {
            image: out.snapshot(),
            progress,
            seconds,
        }
    }
    fn finish(
        &self,
        out: &mut Drawing,
        progress: f64,
        seconds: f64,
        light: LightState,
        options: &Options,
    ) {
        let ambient = if options.ambient_motion {
            seconds * 1000.
        } else {
            0.
        };
        let quantized =
            round(progress * crate::tree::GROWTH_STEPS as f64) / crate::tree::GROWTH_STEPS as f64;
        if options.motes {
            for i in 0..24 {
                let cycle = (ambient / 8000. + i as f64 * 0.61803398875) % 1.;
                let brightness = (cycle * std::f64::consts::PI).sin();
                if brightness < 0.22 {
                    continue;
                }
                let x =
                    round(512. + (i as f64 * 2.399 + cycle * 0.8).sin() * (30. + 340. * quantized));
                let y = round(493. - cycle * (65. + 405. * quantized));
                out.fill(
                    [x, y, 2., 2.],
                    if i % 3 != 0 { "#fff3b2" } else { "#cbffee" },
                    brightness * 0.55,
                    BlendMode::SrcOver,
                );
            }
        }
        if options.foreground {
            let mut plants = Drawing::new(crate::WIDTH as i32, crate::HEIGHT as i32);
            for plant in &self.assets.data.plants {
                plants.image_rect(
                    &self.assets.image(&plant.image),
                    None,
                    plant.rect,
                    1.,
                    BlendMode::SrcOver,
                );
            }
            light.tint(&mut plants, 0.61);
            out.image(&plants.snapshot(), 0., 0.);
        }
    }
}
