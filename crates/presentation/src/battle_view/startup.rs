//! Fixed battle bindings and real renderer pipeline preparation at boot.
use super::*;
use std::{collections::VecDeque, time::Instant};

use crate::gpu_prepare::Shared;
use resonance_game::battle::encounter::CommonModelKind;

pub(crate) fn install(app: &mut App) {
    let pool = ResidentModels::default();
    app.insert_resource(pool.clone())
        .init_resource::<ResidentBindings>();
    app.sub_app_mut(bevy::render::RenderApp)
        .insert_resource(pool);
}

pub(crate) struct Warm {
    next: usize,
    effects: VecDeque<effects::Effects>,
    styles: BTreeSet<ModelStyle>,
    toon: Handle<Image>,
    shared: Shared,
    since: Instant,
}

impl Warm {
    pub(crate) fn new(world: &mut World) -> Result<Option<Self>> {
        let common = world
            .resource::<crate::loading::Resident>()
            .common
            .get()
            .context("common battle missing")?
            .battle
            .clone();
        let Some(common) = common else {
            world.resource::<ResidentModels>().finish_startup();
            world.resource_mut::<ResidentBindings>().finish_startup();
            return Ok(None);
        };
        let shared = Shared::new(world);
        let toon = world
            .resource::<AssetServer>()
            .get_handle::<Image>(resonance_content::model_preview::document::TOON_RAMP)
            .context("common toon image missing")?;
        // Finish fallible discovery before allocating preparation entities.
        let effects = common
            .render_effects()
            .into_iter()
            // Missing optional pages were already diagnosed by CPU capture.
            // Do not request uncaptured files just to populate a warm cache.
            .filter(|bank| {
                bank.textures
                    .values()
                    .flatten()
                    .flat_map(|texture| &texture.images)
                    .all(|image| common.files.contains_key(&image.path))
            })
            .map(|bank| {
                effects::Effects::load(
                    vec![bank],
                    None,
                    world.resource::<AssetServer>(),
                    Diagnostics::default(),
                )
            })
            .collect::<Result<_>>()?;
        let styles = common
            .effect_banks()
            .flat_map(|bank| &bank.actors)
            .filter_map(|declaration| declaration.model_visual().ok())
            .filter(|visual| usize::from(visual.blend) < Blend::ALL.len())
            .flat_map(|visual| {
                [
                    ModelStyle::new(visual, false),
                    ModelStyle::new(visual, true),
                ]
            })
            .collect();
        let mut warm = Self {
            next: 0,
            effects,
            styles,
            toon: toon.clone(),
            shared,
            since: Instant::now(),
        };
        let mesh = world.resource::<crate::materials::GeometryMesh>().0.clone();
        for recipe in effects::startup_surfaces(common.effect_banks().map(Arc::as_ref), &toon) {
            warm.draw(world, &mesh, recipe, false, true);
        }
        // Sprite particles, weapon ribbons and ropes share the streamed layout.
        for (blend, depth_write) in [
            (Blend::Alpha, false),
            (Blend::Additive, false),
            (Blend::Alpha, true),
        ] {
            warm.draw(
                world,
                &mesh,
                TitleSurface {
                    blend: Some(blend),
                    depth_write,
                    depth_equal: true,
                    cull: resonance_content::CullFace::None,
                    ..default()
                },
                false,
                true,
            );
        }
        Ok(Some(warm))
    }

    fn draw(
        &mut self,
        world: &mut World,
        mesh: &Handle<Mesh>,
        recipe: TitleSurface,
        resident: bool,
        streamed: bool,
    ) {
        if resident {
            world.resource_scope(|world, pool: Mut<ResidentModels>| {
                pool.warm(
                    recipe.clone(),
                    &mut world.resource_mut::<Assets<TitleSurface>>(),
                );
            });
        }
        self.shared.0.lock().unwrap().prepare(
            crate::gpu_prepare::View::Battle,
            mesh.id(),
            &recipe,
            streamed,
            None,
        );
    }

    fn model(
        &mut self,
        world: &mut World,
        part: &crate::model_asset::Part,
        additive: bool,
        kind: CommonModelKind,
    ) -> Result<()> {
        let layout = part.layout.as_ref().context("unresolved common model")?;
        let mut recipes = vec![Vec::new(); part.spec.materials.len()];
        let styles = match kind {
            CommonModelKind::Body | CommonModelKind::Particle => vec![(true, None)],
            CommonModelKind::Wings => vec![(false, None)],
            CommonModelKind::Weapon => [false, true]
                .into_iter()
                .flat_map(|toon| {
                    [false, true].map(move |additive| {
                        (
                            true,
                            Some(WeaponStyle {
                                toon,
                                additive,
                                before_body: false,
                            }),
                        )
                    })
                })
                .collect(),
        };
        let body = matches!(kind, CommonModelKind::Body | CommonModelKind::Wings);
        let particle = matches!(kind, CommonModelKind::Particle);
        for (lit, weapon) in styles {
            for (slot, surface) in surface_recipes_from_views(
                &part.spec,
                part.materials
                    .iter()
                    .map(|material| material.sampled.clone()),
                &self.toon,
                lit,
                weapon,
                additive,
                &[],
            )
            .into_iter()
            .enumerate()
            {
                if particle {
                    recipes[slot].extend(
                        self.styles
                            .iter()
                            .map(|style| (style.surface(surface.clone(), &self.toon), false)),
                    );
                } else {
                    let (surface, faded) = model_surface_variants(surface, body);
                    recipes[slot].extend(faded.into_iter().chain([surface]).map(|s| (s, true)));
                }
            }
        }
        for draw in layout.nodes.iter().filter_map(|node| node.draw.as_ref()) {
            let slot = draw.slot.index(recipes.len())?;
            for (recipe, resident) in &recipes[slot] {
                self.draw(world, &draw.mesh, recipe.clone(), *resident, false);
            }
        }
        Ok(())
    }

    pub(crate) fn update(
        &mut self,
        world: &mut World,
        models: &[crate::loading::graphics::Model],
    ) -> Result<bool> {
        {
            let report = self.shared.0.lock().unwrap();
            if let Some(error) = &report.error {
                anyhow::bail!("common battle pipeline: {error}")
            }
            // A finished warmup keeps being polled while other startup stages run.
            if report.completed.load(Ordering::Acquire) {
                return Ok(true);
            }
        }
        ensure!(
            self.since.elapsed().as_secs() < 120,
            "common battle GPU preparation timed out"
        );
        // Materials referencing a rejected image never acquire a GPU binding.
        // Surface the loader error instead of waiting for the submission timeout.
        if !crate::field_view::image_ready(
            world.resource::<AssetServer>(),
            world.resource::<Assets<Image>>(),
            &self.toon,
        )? {
            return Ok(false);
        }
        for _ in 0..8 {
            let Some(effects) = self.effects.front() else {
                break;
            };
            let images_ready = effects.images().try_fold(true, |ready, image| {
                crate::field_view::image_ready(
                    world.resource::<AssetServer>(),
                    world.resource::<Assets<Image>>(),
                    image,
                )
                .map(|loaded| ready && loaded)
            });
            match images_ready {
                Ok(false) => break,
                Ok(true) => {}
                Err(error) => {
                    crate::diagnostics::policy(world).report("common battle graphics", error)?;
                    self.effects.pop_front();
                    continue;
                }
            }
            let mut effects = self.effects.pop_front().unwrap();
            let mut sampled = world.resource::<crate::scene::SampledImages>().clone();
            world.resource_scope(|world, mut surfaces: Mut<Assets<TitleSurface>>| {
                world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
                    effects.prepare_startup(
                        &mut surfaces,
                        &mut images,
                        &mut sampled,
                        world.resource::<ResidentBindings>(),
                    )
                })
            })?;
            effects.retain_bindings(&mut world.resource_mut::<ResidentBindings>());
        }
        for _ in 0..32 {
            let Some(model) = models.get(self.next) else {
                break;
            };
            if let Some(part) = &model.part {
                if part.layout.is_none() {
                    break;
                }
                if let Err(error) = self.model(world, part, model.additive, model.kind) {
                    crate::diagnostics::policy(world).report("common battle graphics", error)?;
                }
            }
            self.next += 1;
        }
        if self.next == models.len() && self.effects.is_empty() {
            let mut report = self.shared.0.lock().unwrap();
            if !report.armed {
                report.armed = true;
            }
        }
        Ok(false)
    }

    #[cfg(all(test, feature = "preparation-trace"))]
    pub(crate) fn pipelines_building(&self) -> bool {
        let report = self.shared.0.lock().unwrap();
        !report.armed && !report.pipelines.is_empty()
    }

    pub(crate) fn retire(&mut self, world: &mut World) {
        world.resource::<ResidentModels>().finish_startup();
        world.resource_mut::<ResidentBindings>().finish_startup();
        info!(
            models = world.resource::<ResidentModels>().len(),
            effects = world.resource::<ResidentBindings>().len(),
            "Common battle GPU bindings resident"
        );
        *self.shared.0.lock().unwrap() = gpu::Report::default();
    }

    pub(crate) fn abort(&mut self, world: &mut World) {
        world.resource::<ResidentModels>().discard_startup();
        world.resource_mut::<ResidentBindings>().discard_startup();
        self.retire(world);
    }
}

#[cfg(test)]
pub(crate) fn check_reuse(world: &World) -> Result<()> {
    let models = world.resource::<ResidentModels>();
    let effects = world.resource::<ResidentBindings>();
    ensure!(
        models.hits() > 0 && effects.textured_hits() > 0 && effects.hits() > 0,
        "battle did not reuse common GPU bindings"
    );
    ResidentBindings::check_retained(world)
}
