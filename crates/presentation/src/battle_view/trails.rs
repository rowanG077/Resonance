//! Weapon ribbons own their short pose history; drawing never advances it.
use super::*;
use crate::native::MeshMaterial3d;
use crate::weapon_geometry::Vertices;
use resonance_battle::WeaponSlot;
use resonance_battle::{ActorId, Cue, ResourceId, WeaponFrame};
use resonance_content::{
    battle_model::TrailMaterial,
    font::UiTexture,
    texture::{Filter, Sampler},
};
use resonance_events::effect::Blend;
use std::collections::VecDeque;

struct Ribbon {
    bones: Vec<u16>,
    material: TrailMaterial,
    textures: Vec<Option<(Handle<Image>, UiTexture, Sampler)>>,
    draws: Vec<Draw>,
}
struct Draw {
    entity: Entity,
    geometry: crate::materials::GeometryData,
}
pub(super) struct Trails {
    diagnostics: Diagnostics,
    ribbons: BTreeMap<ResourceId, Ribbon>,
    history: BTreeMap<(ActorId, WeaponSlot), Trail>,
    ropes: Vec<Draw>,
    ready: bool,
}
impl Trails {
    pub fn load(
        assets: Vec<RenderTrail>,
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut ribbons = BTreeMap::new();
        for asset in assets {
            let resource = asset.resource;
            let result = (|| -> Result<()> {
                ensure!(
                    (2..=3).contains(&asset.bones.len()),
                    "invalid weapon trail endpoints"
                );
                ensure!(
                    (1..=2).contains(&asset.textures.len())
                        && asset
                            .textures
                            .iter()
                            .all(|texture| asset.material.texture.is_some() == texture.is_some()),
                    "battle ribbon texture is missing"
                );
                let textures = asset
                    .textures
                    .into_iter()
                    .map(|texture| {
                        texture
                            .map(|(image, sampler)| -> Result<_> {
                                image.validate()?;
                                sampler.validate()?;
                                let handle = server
                                    .load_builder()
                                    .with_settings(|s: &mut ImageLoaderSettings| {
                                        s.is_srgb = false;
                                        s.sampler = ImageSampler::linear();
                                    })
                                    .load(image.path.clone());
                                Ok((handle, image, sampler))
                            })
                            .transpose()
                    })
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    ribbons
                        .insert(
                            asset.resource,
                            Ribbon {
                                bones: asset.bones,
                                material: asset.material,
                                textures,
                                draws: Vec::new(),
                            }
                        )
                        .is_none(),
                    "duplicate battle ribbon resource"
                );
                Ok(())
            })();
            if let Err(error) = result {
                diagnostics.report(&format!("battle ribbon {resource}"), error)?;
            }
        }
        Ok(Self {
            diagnostics,
            ribbons,
            history: BTreeMap::new(),
            ropes: Vec::new(),
            ready: false,
        })
    }
    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.ribbons
            .values()
            .flat_map(|r| &r.textures)
            .filter_map(|texture| texture.as_ref().map(|(h, _, _)| h))
    }
    pub fn images_mut(&mut self) -> impl Iterator<Item = &mut Handle<Image>> {
        self.ribbons
            .values_mut()
            .flat_map(|r| &mut r.textures)
            .filter_map(|texture| texture.as_mut().map(|(h, _, _)| h))
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.ribbons
            .values()
            .flat_map(|r| &r.draws)
            .chain(&self.ropes)
            .map(|d| d.entity)
    }
    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        assets: &mut AssetsForView,
        sampled: &mut crate::scene::SampledImages,
        report: &mut gpu::Report,
    ) -> Result<()> {
        if self.ready {
            return Ok(());
        }
        for ribbon in self.ribbons.values_mut() {
            for texture in &ribbon.textures {
                let color = texture.as_ref().map(|(image, _, sampler)| {
                    let binding = resonance_content::TextureBinding {
                        texture: 0,
                        wrap_u: sampler.wrap[0],
                        wrap_v: sampler.wrap[1],
                        nearest_min: matches!(
                            sampler.min_filter,
                            Filter::Nearest
                                | Filter::NearestMipmapNearest
                                | Filter::NearestMipmapLinear
                        ),
                        nearest_mag: matches!(sampler.mag_filter, Filter::Nearest),
                        mipmapped: false,
                        scale: resonance_content::TextureBinding::unscaled(),
                    };
                    crate::scene::sampled_image(
                        Some((image.clone(), binding)),
                        &mut assets.images,
                        sampled,
                    )
                    .unwrap()
                });
                let surface = assets.surfaces.add(TitleSurface {
                    blend: Some(if ribbon.material.additive {
                        Blend::Additive
                    } else {
                        Blend::Alpha
                    }),
                    depth_write: false,
                    depth_equal: true,
                    cull: resonance_content::CullFace::None,
                    ..TitleSurface::textured(color)
                });
                ribbon.draws.push(Draw::prepare(
                    commands,
                    &assets.geometry_mesh.0,
                    surface,
                    &assets.surfaces,
                    report,
                ));
            }
        }
        let surface = assets.surfaces.add(TitleSurface {
            blend: Some(Blend::Alpha),
            // Enable ordinary weapon depth writes before drawing the ribbon.
            depth_write: true,
            depth_equal: true,
            cull: resonance_content::CullFace::None,
            ..default()
        });
        // Eight native actors, each with at most two ordinary weapon slots.
        for _ in 0..16 {
            self.ropes.push(Draw::prepare(
                commands,
                &assets.geometry_mesh.0,
                surface.clone(),
                &assets.surfaces,
                report,
            ));
        }
        self.ready = true;
        Ok(())
    }

    pub fn apply(
        &self,
        frame: &BattleFrame,
        sample: &super::interpolation::Input,
        commands: &mut Commands,
        order: &ActorOrder,
        models: &BTreeMap<ResourceId, super::Model>,
    ) -> Result<()> {
        for entity in self.entities() {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        for (&(actor, slot), trail) in &self.history {
            if trail.samples.is_empty() || !super::actor_visible(frame, actor) {
                continue;
            }
            let result = (|| -> Result<()> {
                let ribbon = &self.ribbons[&trail.resource];
                let variant = usize::from(trail.highlighted);
                let size = ribbon
                    .textures
                    .get(variant)
                    .context("weapon trail material was not prepared")?
                    .as_ref()
                    .map_or([1, 1], |(_, image, _)| [image.width, image.height]);
                let style = models
                    .get(&trail.resource)
                    .context("missing trail weapon model")?
                    .weapon_style;
                let weapon = frame
                    .weapons
                    .iter()
                    .find(|weapon| weapon.owner == actor && weapon.slot == Some(slot));
                let step = weapon.and_then(|w| trail.predicted_step(ribbon, sample, w));
                let vertices = trail.vertices(ribbon, size, step.as_ref(), sample.alpha, weapon)?;
                if vertices.is_empty() {
                    return Ok(());
                }
                ribbon
                    .draws
                    .get(variant)
                    .context("weapon trail was not prepared")?
                    .apply(
                        vertices,
                        DrawOrder(
                            weapon_layer(order.get(actor)?, Some(slot), style, true),
                            0,
                            0,
                        ),
                        commands,
                    );
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics
                    .report("battle weapon trail drawing", error)?;
            }
        }
        let camera = frame.camera.context("battle rope has no camera")?;
        let fov = frame
            .camera_fov_degrees
            .unwrap_or(resonance_battle::VERTICAL_FOV_DEGREES)
            .to_radians();
        let eye = Vec3::from_array(camera.eye);
        let rotation = Transform::from_translation(eye)
            .looking_at(Vec3::from_array(camera.focus), Vec3::Y)
            .rotation;
        let mut rope_slot = 0;
        for weapon in &frame.weapons {
            if !weapon.visible
                || !super::actor_visible(frame, weapon.owner)
                || weapon.links.is_empty()
            {
                continue;
            }
            let result = (|| -> Result<()> {
                let draw = self
                    .ropes
                    .get(rope_slot)
                    .context("battle rope exceeds prepared weapon slots")?;
                rope_slot += 1;
                let actor = frame
                    .actors
                    .get(weapon.owner.index())
                    .context("invalid battle rope owner")?;
                let distance = eye.distance(Vec3::from_array(actor.position));
                let width = crate::weapon_geometry::string_width(distance);
                let world = Mat4::from_cols_array_2d(&weapon.world);
                let mut data = Vertices::default();
                for &[a, b] in &weapon.links {
                    let endpoint = |bone| -> Result<Vec3> {
                        let bone = weapon
                            .bones
                            .get(usize::from(bone))
                            .context("invalid battle rope bone")?;
                        Ok((world * Mat4::from_cols_array_2d(bone)).transform_point3(Vec3::ZERO))
                    };
                    data.line(endpoint(a)?, endpoint(b)?, eye, rotation, width, fov);
                }
                if data.is_empty() {
                    return Ok(());
                }
                draw.apply(
                    data,
                    DrawOrder(
                        weapon_layer(
                            order.get(weapon.owner)?,
                            weapon.slot,
                            models
                                .get(&weapon.resource)
                                .context("missing battle rope model")?
                                .weapon_style,
                            true,
                        ),
                        0,
                        0,
                    ),
                    commands,
                );
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics
                    .report(&format!("battle rope draw {}", weapon.resource), error)?;
            }
        }
        Ok(())
    }
    pub fn despawn(self, commands: &mut Commands) {
        for entity in self.entities() {
            commands.entity(entity).despawn();
        }
    }
}
impl Draw {
    fn prepare(
        commands: &mut Commands,
        mesh: &Handle<Mesh>,
        material: Handle<TitleSurface>,
        surfaces: &Assets<TitleSurface>,
        report: &mut gpu::Report,
    ) -> Self {
        let geometry = crate::materials::GeometryData::default();
        report.prepare(
            gpu::View::Battle,
            mesh.id(),
            surfaces.get(&material).expect("prepared trail material"),
            true,
            None,
        );
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                geometry.clone(),
                Transform::default(),
                Visibility::Hidden,
                NoFrustumCulling,
                RenderLayers::layer(LAYER),
                DrawOrder(Layer::Effects, 0, 0),
            ))
            .id();
        Self { entity, geometry }
    }
    fn apply(&self, vertices: Vertices, order: DrawOrder, commands: &mut Commands) {
        self.geometry.replace(vertices.packed());
        commands
            .entity(self.entity)
            .insert((Visibility::Inherited, order));
    }
}
const HISTORY_SAMPLES: usize = 8;
// Sub-tenth-unit endpoint drift is a held pose, not a new motion sample.
const STILL_DISTANCE: f32 = 0.1;
// Sixteen opacity steps fade the retained path after its activation ends.
const FADE_UPDATES: u8 = 16;

struct Trail {
    resource: ResourceId,
    remaining: u16,
    highlighted: bool,
    fade: u8,
    samples: VecDeque<Vec<Vec3>>,
}

enum RibbonStep {
    Insert(Vec<Vec3>),
    Drop { fade: u8 },
}

impl Trail {
    fn step(&self, ribbon: &Ribbon, weapon: &WeaponFrame, active: bool) -> Result<RibbonStep> {
        if !active {
            return Ok(RibbonStep::Drop {
                fade: self.fade.saturating_add(1),
            });
        }
        let points = ribbon.sample(weapon)?;
        let held = self.samples.front().is_some_and(|head| {
            head.iter()
                .zip(&points)
                .all(|(a, b)| (*a - *b).abs().max_element() < STILL_DISTANCE)
        });
        Ok(if held {
            RibbonStep::Drop { fade: 0 }
        } else {
            RibbonStep::Insert(points)
        })
    }

    fn predicted_step(
        &self,
        ribbon: &Ribbon,
        sample: &super::interpolation::Input,
        current: &WeaponFrame,
    ) -> Option<RibbonStep> {
        let next = sample.next?;
        let actor = current.owner;
        let slot = current.slot?;
        if !current.visible
            || next.clock.feedback_paused()
            || next.paused_actors.contains(&actor)
            || next.recognized_result.is_some()
            || next.outcome.is_some()
            || next.cues.iter().any(|cue| matches!(cue, Cue::ClearEffects))
        {
            return None;
        }
        let owner_continues = || {
            let from = sample.source.models.iter().find(|m| m.actor == actor);
            from.is_some_and(|a| {
                next.models
                    .iter()
                    .any(|b| super::interpolation::continuous_model(a, b))
            })
        };
        let weapon = next.weapons.iter().find(|w| {
            w.resource == self.resource
                && super::interpolation::continuous_weapon(current, w, owner_continues)
        })?;
        let active = self.remaining > 0 || next.cues.iter().any(|cue| {
            matches!(cue, Cue::WeaponTrail { actor: owner, slot: hand, duration: 1.., .. } if *owner == actor && *hand == slot)
        });
        self.step(ribbon, weapon, active).ok()
    }

    fn vertices(
        &self,
        ribbon: &Ribbon,
        size: [u32; 2],
        step: Option<&RibbonStep>,
        alpha: f32,
        weapon: Option<&WeaponFrame>,
    ) -> Result<Vertices> {
        let from = RibbonPath {
            history: &self.samples,
            head: None,
            count: self.samples.len(),
            fade: self.fade,
        };
        let to = match step {
            None => from,
            Some(RibbonStep::Insert(points)) => RibbonPath {
                head: Some(points),
                count: (from.count + 1).min(HISTORY_SAMPLES),
                fade: 0,
                ..from
            },
            Some(RibbonStep::Drop { fade }) => RibbonPath {
                count: from.count.saturating_sub(1),
                fade: *fade,
                ..from
            },
        };
        let head = if to.head.is_some() {
            weapon.map(|w| ribbon.sample(w)).transpose()?
        } else {
            None
        };
        Ok(ribbon_vertices(
            from,
            to,
            head.as_deref(),
            &ribbon.material,
            size,
            self.highlighted,
            alpha,
        ))
    }
}

impl Ribbon {
    fn sample(&self, weapon: &WeaponFrame) -> Result<Vec<Vec3>> {
        let world = Mat4::from_cols_array_2d(&weapon.world);
        self.bones
            .iter()
            .map(|&bone| {
                let pose = weapon
                    .bones
                    .get(usize::from(bone))
                    .context("invalid weapon trail bone")?;
                let point = (world * Mat4::from_cols_array_2d(pose)).transform_point3(Vec3::ZERO);
                ensure!(point.is_finite(), "invalid weapon trail pose");
                Ok(point)
            })
            .collect()
    }
}

impl Trails {
    /// Consume a completed frame once. Commands and gameplay pauses hold all trail feedback.
    pub fn advance(&mut self, frame: &BattleFrame, paused: bool) -> Result<()> {
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::ClearEffects))
        {
            self.history.clear();
        }
        if frame.recognized_result.is_some() || frame.outcome.is_some() {
            self.history.clear();
            return Ok(());
        }
        let weapon = |actor, slot| {
            frame.weapons.iter().find(|weapon| {
                weapon.owner == actor
                    && weapon.slot == Some(slot)
                    && self.ribbons.contains_key(&weapon.resource)
            })
        };
        for cue in &frame.cues {
            if let Cue::WeaponTrail {
                actor,
                slot,
                duration,
                highlighted,
            } = *cue
                && duration > 0
                && let Some(weapon) = weapon(actor, slot)
            {
                let trail = self.history.entry((actor, slot)).or_insert_with(|| Trail {
                    resource: weapon.resource,
                    remaining: 0,
                    highlighted,
                    fade: 0,
                    samples: VecDeque::new(),
                });
                trail.remaining = trail.remaining.max(duration);
                trail.highlighted = highlighted;
            }
        }
        for (&(actor, slot), trail) in &mut self.history {
            let Some(weapon) = weapon(actor, slot).filter(|weapon| weapon.visible) else {
                trail.remaining = 0;
                trail.samples.clear();
                continue;
            };
            if trail.resource != weapon.resource {
                trail.resource = weapon.resource;
                trail.samples.clear();
                trail.fade = 0;
            }
            if paused || frame.paused_actors.contains(&actor) {
                continue;
            }
            let step = trail.step(&self.ribbons[&trail.resource], weapon, trail.remaining > 0);
            trail.remaining = trail.remaining.saturating_sub(1);
            match step {
                Ok(RibbonStep::Insert(points)) => {
                    trail.fade = 0;
                    trail.samples.push_front(points);
                    trail.samples.truncate(HISTORY_SAMPLES);
                }
                Ok(RibbonStep::Drop { fade }) => {
                    trail.fade = fade;
                    trail.samples.pop_back();
                }
                Err(error) => {
                    trail.remaining = 0;
                    trail.samples.clear();
                    self.diagnostics.report("battle weapon trail pose", error)?;
                }
            }
        }
        self.history
            .retain(|_, trail| trail.remaining > 0 || !trail.samples.is_empty());
        Ok(())
    }
}

/// The predicted path borrows the committed history and changes at most one pose at each end.
#[derive(Clone, Copy)]
struct RibbonPath<'a> {
    history: &'a VecDeque<Vec<Vec3>>,
    head: Option<&'a [Vec3]>,
    count: usize,
    fade: u8,
}

impl RibbonPath<'_> {
    fn sample(self, index: usize, row: usize) -> Vec3 {
        match self.head {
            Some(head) if index == 0 => head[row],
            _ => self.history[index - usize::from(self.head.is_some())][row],
        }
    }

    fn point(self, row: usize, distance: f32) -> Vec3 {
        let last = self.count.saturating_sub(1);
        let i = distance as usize;
        if i >= last {
            return self.sample(last, row);
        }
        let start = self.sample(i, row);
        let end = self.sample(i + 1, row);
        let prev = i
            .checked_sub(1)
            .map_or_else(|| 2. * start - end, |j| self.sample(j, row));
        let next = if i + 1 < last {
            self.sample(i + 2, row)
        } else {
            2. * end - start
        };
        curved_edge(prev, start, end, next, distance.fract())
    }

    fn along(self, distance: f32) -> f32 {
        (distance / self.count.saturating_sub(1).max(1) as f32).min(1.)
    }

    fn opacity(self, along: f32) -> f32 {
        if self.count < 2 {
            0.
        } else {
            (1. - along - f32::from(self.fade) / f32::from(FADE_UPDATES)).max(0.)
        }
    }
}

fn ribbon_vertices(
    from: RibbonPath,
    to: RibbonPath,
    visual_head: Option<&[Vec3]>,
    material: &TrailMaterial,
    size: [u32; 2],
    highlighted: bool,
    alpha: f32,
) -> Vertices {
    let mut data = Vertices::default();
    let Some(first) = from.history.front() else {
        return data;
    };
    let inserted = usize::from(to.head.is_some());
    let [u, v, width, height] = material.uv.map(f32::from);
    let rgb = if highlighted { [1.; 3] } else { material.color };
    let vertex = |row: usize, d: f32| {
        // A new interval grows from the old tip. A discarded interval contracts to the new tail.
        let old = (d - inserted as f32).max(0.);
        let mut point = from.point(row, old).lerp(to.point(row, d), alpha);
        if let Some(head) = visual_head
            && d < 1.
        {
            let linear_head = first[row].lerp(to.sample(0, row), alpha);
            point += (head[row] - linear_head) * (1. - d);
        }
        let (a, b) = (from.along(old), to.along(d));
        let uv = [
            (u + width * a.lerp(b, alpha)) / size[0] as f32,
            (v + height * (1. - row as f32 / (first.len() - 1) as f32)) / size[1] as f32,
        ];
        let opacity = from.opacity(a).lerp(to.opacity(b), alpha);
        (point, uv, [rgb[0], rgb[1], rgb[2], opacity])
    };
    const CURVE_STEPS: usize = 3;
    let intervals = from.count + inserted - 1;
    for row in 0..first.len() - 1 {
        let mut left = [vertex(row, 0.), vertex(row + 1, 0.)];
        for step in 1..=intervals * CURVE_STEPS {
            let d = step as f32 / CURVE_STEPS as f32;
            let right = [vertex(row, d), vertex(row + 1, d)];
            let quad = [left[0], right[0], left[1], right[1]];
            data.quad(quad.map(|v| v.0), quad.map(|v| v.1), quad.map(|v| v.2));
            left = right;
        }
    }
    data
}

// Cubic Hermite interpolation keeps every sampled pose and joins their tangents.
fn curved_edge(previous: Vec3, start: Vec3, end: Vec3, next: Vec3, t: f32) -> Vec3 {
    if start == end {
        return start;
    }
    let first = (end - previous) * 0.5;
    let last = (next - start) * 0.5;
    start
        + t * (first
            + t * (3. * (end - start) - 2. * first - last
                + t * (2. * (start - end) + first + last)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{BattleResult, ModelMaterial, Side};
    use std::sync::Arc;

    #[test]
    fn weapon_ribbons_follow_equipment_hold_fade_and_omit_bad_art() -> Result<()> {
        let mut frame = crate::test_support::prepare_battle([crate::test_support::actor(
            Side::Party,
            100,
            20,
        )])?
        .finish()?
        .snapshot();
        let actor = ActorId::from_index(0)?;
        let material = TrailMaterial {
            texture: None,
            palette: 0,
            additive: false,
            color: [0.5, 0.25, 0.25],
            uv: [0, 0, 64, 64],
        };
        let mut trails = Trails {
            diagnostics: Diagnostics::new(true),
            ribbons: [(7, vec![1, 0]), (8, vec![2, 0, 1])]
                .into_iter()
                .map(|(id, bones)| {
                    (
                        ResourceId(id),
                        Ribbon {
                            bones,
                            material,
                            textures: vec![None, None],
                            draws: Vec::new(),
                        },
                    )
                })
                .collect(),
            history: BTreeMap::new(),
            ropes: Vec::new(),
            ready: false,
        };
        frame.weapons.push(WeaponFrame {
            attachment: None,
            texture: None,
            owner: actor,
            slot: Some(WeaponSlot(0)),
            visible: true,
            tint: [255; 4],
            material: ModelMaterial::Normal,
            resource: ResourceId(7),
            clip: None,
            frame: 0.,
            world: Mat4::from_translation(Vec3::new(10., 20., 30.)).to_cols_array_2d(),
            bones: Arc::new(resonance_content::animation::pose::Palette::independent(
                vec![
                    Mat4::IDENTITY.to_cols_array_2d(),
                    Mat4::from_translation(Vec3::new(2., 0., 0.)).to_cols_array_2d(),
                    Mat4::from_translation(Vec3::new(0., 2., 0.)).to_cols_array_2d(),
                ],
            )),
            links: vec![],
        });
        let request = Cue::WeaponTrail {
            actor,
            slot: WeaponSlot(0),
            duration: 20,
            highlighted: false,
        };
        frame.cues = vec![request.clone()];
        trails.advance(&frame, false)?;
        frame.cues.clear();
        frame.weapons[0].world[3][0] += 2.;
        trails.advance(&frame, false)?;
        let key = (actor, WeaponSlot(0));
        let vertices = |trails: &Trails| -> Result<Vec<_>> {
            let trail = &trails.history[&key];
            let ribbon = &trails.ribbons[&trail.resource];
            Ok(trail
                .vertices(ribbon, [256; 2], None, 0., None)?
                .packed()
                .collect())
        };
        let before = vertices(&trails)?;
        for x in [12., 14.] {
            assert!(before.iter().any(|v| v.position == [x, 20., 30., 1.]));
        }
        for uv in [[0., 0.], [0.25, 0.25]] {
            assert!(before.iter().any(|v| v.uv[..2] == uv));
        }
        // Render-only sword movement must move the ribbon's leading edge without
        // adding history samples, consuming duration, or advancing its fade.
        let trail = &trails.history[&key];
        let ribbon = &trails.ribbons[&trail.resource];
        let mut next = frame.weapons[0].clone();
        next.world[3][2] += 4.;
        let step = trail.step(ribbon, &next, true)?;
        for alpha in [0.25, 0.75, 1.] {
            let mut visual = frame.weapons[0].clone();
            visual.world[3][2] += 4. * alpha;
            let sampled = trail.vertices(ribbon, [256; 2], Some(&step), alpha, Some(&visual))?;
            let mut vertices = sampled.packed();
            let tip = vertices.next().unwrap().position;
            assert_eq!(&tip[..3], &ribbon.sample(&visual)?[0].to_array());
            // The bend at the previous sword tip remains in the ribbon throughout the turn.
            assert_eq!(
                &vertices.nth(17).unwrap().position[..3],
                &trail.samples[0][0].to_array()
            );
        }
        trails.advance(&frame, true)?;
        assert_eq!(before, vertices(&trails)?);
        frame.cues = vec![Cue::WeaponTrail {
            actor,
            slot: WeaponSlot(0),
            duration: 20,
            highlighted: true,
        }];
        trails.advance(&frame, true)?;
        assert!(trails.history[&key].highlighted);
        assert_eq!(vertices(&trails)?[0].color[..3], [1.; 3]);
        assert_eq!(before[0].color[..3], [0.5, 0.25, 0.25]);
        assert_eq!(trails.history[&key].samples.len(), 2);
        frame.cues.clear();
        // Finishing the swing must retain a visible arc across a held pose.
        for _ in 0..6 {
            frame.weapons[0].world[3][2] += 2.;
            let trail = &trails.history[&key];
            let ribbon = &trails.ribbons[&trail.resource];
            let sword = &frame.weapons[0];
            let step = trail.step(ribbon, sword, true)?;
            let end = trail.vertices(ribbon, [256; 2], Some(&step), 1., Some(sword))?;
            trails.advance(&frame, false)?;
            for (a, b) in end.packed().zip(vertices(&trails)?) {
                assert!(
                    Vec3::from_slice(&a.position).abs_diff_eq(Vec3::from_slice(&b.position), 1e-5)
                );
                assert!(Vec4::from_array(a.uv).abs_diff_eq(Vec4::from_array(b.uv), 1e-5));
            }
        }
        for _ in 0..4 {
            trails.advance(&frame, false)?;
        }
        let held_swing = vertices(&trails)?;
        assert!(held_swing.as_chunks::<3>().0.iter().any(|triangle| {
            let [a, b, c] = triangle.map(|v| Vec3::from_slice(&v.position));
            (b - a).cross(c - a).length_squared() > 0.01
        }));
        let remaining = trails.history[&key].remaining;
        frame.paused_actors.push(actor);
        trails.advance(&frame, false)?;
        assert_eq!(trails.history[&key].remaining, remaining);
        assert_eq!(vertices(&trails)?, held_swing);
        frame.paused_actors.clear();
        for _ in 0..4 {
            trails.advance(&frame, false)?;
        }
        assert!(trails.history[&key].samples.len() < 2);
        // Switching a held weapon must never connect its new pose to the old blade.
        frame.weapons[0].resource = ResourceId(8);
        frame.weapons[0].world[3][0] = 300.;
        trails.advance(&frame, true)?;
        trails.advance(&frame, false)?;
        frame.weapons[0].world[3][0] += 2.;
        trails.advance(&frame, false)?;
        assert!(vertices(&trails)?.iter().all(|v| v.position[0] >= 300.));
        // A third ribbon endpoint can move while its first two remain fixed.
        Arc::make_mut(&mut frame.weapons[0].bones).global[1][3][2] += 2.;
        trails.advance(&frame, false)?;
        let trail = &trails.history[&key];
        assert_eq!(trail.samples.len(), 3);
        let ribbon = &trails.ribbons[&trail.resource];
        // The middle sample moves from half the atlas width to its far edge.
        for active in [true, false] {
            let step = trail.step(ribbon, &frame.weapons[0], active)?;
            for t in [0., 0.5, 1.] {
                let mesh = trail.vertices(ribbon, [256; 2], Some(&step), t, None)?;
                let mut vertices = mesh.packed();
                let head = vertices.next().unwrap();
                assert_eq!(
                    head.color[3],
                    if active {
                        1.
                    } else {
                        1. - t / f32::from(FADE_UPDATES)
                    }
                );
                assert_eq!(vertices.nth(17).unwrap().uv[0], 0.125 + 0.125 * t);
            }
        }
        for _ in 0..30 {
            trails.advance(&frame, false)?;
        }
        assert!(trails.history.is_empty());
        // Malformed optional artwork is isolated from simulation in tolerant mode.
        trails.ribbons.get_mut(&ResourceId(8)).unwrap().bones[0] = u16::MAX;
        frame.cues = vec![request.clone()];
        for paranoid in [false, true] {
            trails.diagnostics = Diagnostics::new(paranoid);
            assert_eq!(trails.advance(&frame, false).is_err(), paranoid);
            assert!(trails.diagnostics.has_errors());
            assert!(
                trails
                    .history
                    .values()
                    .all(|trail| trail.samples.is_empty())
            );
        }
        trails.ribbons.get_mut(&ResourceId(8)).unwrap().bones[0] = 2;
        trails.advance(&frame, false)?;
        frame.weapons[0].visible = false;
        trails.advance(&frame, true)?;
        assert!(trails.history.is_empty());
        frame.weapons[0].visible = true;
        trails.advance(&frame, false)?;
        frame.recognized_result = Some(BattleResult::Victory);
        trails.advance(&frame, true)?;
        assert!(trails.history.is_empty());
        Ok(())
    }
}
