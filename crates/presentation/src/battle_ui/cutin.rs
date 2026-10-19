//! A skill portrait uses the ordinary HUD surface and its owning action's lifetime.
use super::*;
use resonance_battle::{ActionId, Cue};

const PORTRAIT_TOP: f32 = 40.;
const VIEWPORT_WIDTH: f32 = 640.;
const FADE_TICKS: u16 = 16;
const SLIDE_DISTANCE: f32 = 87.5;

#[derive(Default)]
pub(super) struct State {
    pub(super) layers: BTreeMap<u16, HudLayer>,
    active: Option<Active>,
    last_update: Option<u64>,
}

struct Active {
    action: ActionId,
    catalogue: u16,
    age: u16,
    duration: u16,
}

impl Artwork {
    pub(crate) fn load_cutins(
        &mut self,
        files: &resonance_content::prepared::Files,
        scenes: impl IntoIterator<Item = (u16, String)>,
        materials: &mut Assets<Surface>,
        server: &AssetServer,
    ) -> Result<()> {
        for (catalogue, path) in scenes {
            let scene: resonance_content::battle_scene::Scene = files.json(&path)?;
            let Some(cutin) = scene.cutin else { continue };
            ensure!(cutin.images.len() == 1, "skill cut-in requires one image");
            let texture = &cutin.images[0];
            let image = menu::menu_image(server, texture.path.clone(), false);
            self.pending_images.push(PendingImage {
                image: image.clone(),
                size: [texture.width, texture.height],
                required: true,
            });
            self.cutin.layers.insert(
                catalogue,
                HudLayer::load(
                    LayerDefinition::new(
                        image.clone(),
                        [texture.width, texture.height],
                        Depth::Cutin,
                    ),
                    &image,
                    materials,
                ),
            );
        }
        Ok(())
    }
}

impl State {
    pub(super) fn advance(&mut self, frame: &BattleFrame) -> Result<()> {
        // Portraits advance during cinematic holds; a user pause produces no new update.
        let elapsed = u16::from(
            self.last_update
                .is_some_and(|update| update != frame.update),
        );
        self.last_update = Some(frame.update);
        if let Some(active) = &mut self.active {
            active.age = active.age.saturating_add(elapsed);
            if active.age >= active.duration
                || !frame.actions.iter().any(|&(id, _, _)| id == active.action)
                || frame.cues.iter().any(|cue| matches!(cue,
                    Cue::Interrupted { action } | Cue::Completed { action } if *action == active.action))
            {
                self.active = None;
            }
        }
        for cue in &frame.cues {
            if let Cue::Cutin {
                action,
                catalogue,
                duration,
            } = cue
            {
                ensure!(
                    self.layers.contains_key(catalogue),
                    "unprepared skill cut-in {catalogue}"
                );
                self.active = (*duration != 0
                    && frame.actions.iter().any(|&(id, _, _)| id == *action))
                .then_some(Active {
                    action: *action,
                    catalogue: *catalogue,
                    age: 0,
                    duration: *duration,
                });
            }
        }
        Ok(())
    }

    pub(super) fn render(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        for (&catalogue, layer) in &mut self.layers {
            let mut batch = Batch::default();
            if let Some(active) = self
                .active
                .as_ref()
                .filter(|active| active.catalogue == catalogue)
            {
                let [width, height] = layer.definition.size;
                let height = height as f32 * VIEWPORT_WIDTH / width as f32;
                let exit_at = active.duration.saturating_sub(FADE_TICKS);
                let (offset, alpha) = if active.age < FADE_TICKS {
                    let progress = f32::from(active.age) / f32::from(FADE_TICKS);
                    (SLIDE_DISTANCE * (1. - progress * progress), progress)
                } else if active.age >= exit_at {
                    let progress = f32::from(active.age - exit_at) / f32::from(FADE_TICKS);
                    let alpha = f32::from(active.duration.saturating_sub(active.age + 1))
                        / f32::from(FADE_TICKS);
                    (SLIDE_DISTANCE * progress * progress, alpha)
                } else {
                    (0., 1.)
                };
                for x in [-offset, offset] {
                    batch.quad(
                        [x, PORTRAIT_TOP, x + VIEWPORT_WIDTH, PORTRAIT_TOP + height],
                        [0., 0., width as f32, layer.definition.size[1] as f32],
                        [1., 1., 1., alpha],
                    );
                }
            }
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}
