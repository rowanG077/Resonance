//! Sample scenery and attachments only when the committed or predicted visit changes.
use super::*;
use crate::sparse_animation::affine::Matrices;

pub(crate) struct Input<'a> {
    pub source: &'a BattleFrame,
    pub next: Option<&'a BattleFrame>,
    pub revision: (u32, u64),
    pub alpha: f32,
}
struct Accessory {
    tint: ([u8; 4], [u8; 4]),
    uv: Option<(usize, [f32; 2], [f32; 2])>,
    bones: Matrices,
    source: Arc<resonance_game::battle::encounter::ModelAccessory>,
}

struct SparklePair {
    from: Vec3,
    to: Vec3,
    alpha: [f32; 2],
}

#[derive(Default)]
pub(super) struct Cache {
    key: Option<((u32, u64), bool)>,
    scenery: BTreeMap<u8, Matrices>,
    attachments: Vec<Accessory>,
    colors: BTreeMap<u8, ([u8; 4], [u8; 4])>,
    pub scenery_poses: BTreeMap<u8, Vec<Matrix>>,
    pub accessories: Vec<(resonance_battle::ResourceId, Key, accessories::Frame)>,
    pub independent: Vec<[f32; 3]>,
    pub next_tick: u64,
    sparkles_pairs: Vec<SparklePair>,
    pub sparkles: Vec<accessories::Sparkle>,
}
impl Cache {
    pub fn sample(
        &mut self,
        view: &mut View,
        input: &Input<'_>,
        rendered: &BattleFrame,
    ) -> Result<()> {
        if self.key != Some((input.revision, input.next.is_some())) {
            self.prepare(view, input)?;
        }
        let alpha = input.alpha;
        for (&slot, matrices) in &mut self.scenery {
            matrices.sample(alpha, self.scenery_poses.get_mut(&slot).unwrap());
        }
        self.independent.clear();
        for (pair, (_, key, out)) in self.attachments.iter_mut().zip(&mut self.accessories) {
            pair.bones.sample(alpha, &mut out.bones.global);
            out.world = accessories::placement(
                &pair.source,
                rendered
                    .models
                    .iter()
                    .find(|model| model.actor == pair.source.owner),
            )?
            .to_cols_array_2d();
            out.tint = color(pair.tint.0, pair.tint.1, alpha);
            if let Some((texture, a, b)) = pair.uv {
                let uv = crate::field_view::prediction::uv_offsets(
                    Vec4::new(a[0], a[1], 0., 0.),
                    Vec4::new(b[0], b[1], 0., 0.),
                    alpha,
                );
                out.uv = Some((texture, [uv.x, uv.y]));
            }
            if matches!(key, Key::Independent(_)) {
                self.independent.push(out.world[3][..3].try_into().unwrap());
            }
        }
        for (pair, out) in self.sparkles_pairs.iter().zip(&mut self.sparkles) {
            out.position = pair.from.lerp(pair.to, alpha);
            // The output keeps the committed atlas palette and lifetime.
            out.opacity = 1.;
            let base = out.alpha();
            out.opacity = (pair.alpha[0] + (pair.alpha[1] - pair.alpha[0]) * alpha) / base;
        }
        Ok(())
    }
    pub fn color(&self, slot: u8, alpha: f32) -> [u8; 4] {
        let (a, b) = self.colors[&slot];
        color(a, b, alpha)
    }
    fn prepare(&mut self, view: &mut View, input: &Input<'_>) -> Result<()> {
        let source = input.source;
        let next = input.next.unwrap_or(source);
        let elapsed = if view.scenery_update != Some(source.update) && !source.clock.paused() {
            view.scenery_update
                .map_or(0, |last| source.update.saturating_sub(last))
        } else {
            0
        };
        view.scenery_tick += elapsed;
        view.scenery_update = Some(source.update);
        self.next_tick = view.scenery_tick
            + if !next.clock.paused() {
                next.update.saturating_sub(source.update)
            } else {
                0
            };
        self.scenery.clear();
        self.scenery_poses.clear();
        self.attachments.clear();
        self.accessories.clear();
        self.colors.clear();
        self.sparkles_pairs.clear();
        self.sparkles.clear();
        for (&slot, model) in &view.scenery_models {
            let from = model.pose(view.scenery_tick)?;
            let to = if self.next_tick == view.scenery_tick {
                from.clone()
            } else {
                model.pose(self.next_tick)?
            };
            self.scenery.insert(slot, Matrices::new(&from, &to));
            self.scenery_poses.insert(slot, from.global);
        }
        let mut next_color = view.stage_color.clone();
        if input.next.is_some() {
            next_color.advance(next.clock, &next.cues);
            next_color.advance_unison(next.clock, next.unison.as_ref());
        }
        for (slot, part) in &view.scenery {
            let mut from = view.stage_color.color();
            let mut to = next_color.color();
            if let Some(fade) = &view.stage.scenery_fade
                && let Some(&minimum) = fade.minimum_alpha.get(slot)
            {
                let alpha = view.scenery_alpha.entry(*slot).or_insert(u8::MAX);
                let point = |future: bool| {
                    let attachment = match view.stage.scenery_bones.get(slot) {
                        Some(&bone) => Mat4::from_cols_array_2d(
                            &self.scenery[&0].endpoint(usize::from(bone), future),
                        ),
                        None => Mat4::IDENTITY,
                    };
                    (stage_world(&view.stage)
                        * attachment
                        * Mat4::from_translation(Vec3::from_array(part.spec.scene.translation)))
                    .transform_point3(Vec3::ZERO)
                };
                *alpha = fade_alpha(*alpha, minimum, fade.step, elapsed, source, point(false))?;
                let future = fade_alpha(
                    *alpha,
                    minimum,
                    fade.step,
                    self.next_tick - view.scenery_tick,
                    next,
                    point(true),
                )?;
                from[3] = (u16::from(from[3]) * u16::from(*alpha) / 255) as u8;
                to[3] = (u16::from(to[3]) * u16::from(future) / 255) as u8;
            }
            self.colors.insert(*slot, (from, to));
        }
        let mut independent = 0;
        for state in &mut view.accessories {
            let from = state.sample(source)?;
            let (future, to) = state.predict(source, next)?;
            if state.source.sparkle_texture().is_some() {
                let mut pairs = BTreeMap::<
                    _,
                    (Option<&accessories::Sparkle>, Option<&accessories::Sparkle>),
                >::new();
                for particle in state.sparkles() {
                    pairs.entry(particle.id).or_default().0 = Some(particle);
                }
                for particle in future.sparkles() {
                    pairs.entry(particle.id).or_default().1 = Some(particle);
                }
                for (_, (a, b)) in pairs {
                    let source = a.or(b).unwrap();
                    let target = b.unwrap_or(source);
                    self.sparkles_pairs.push(SparklePair {
                        from: source.position,
                        to: target.position,
                        alpha: [
                            a.map_or(0., accessories::Sparkle::alpha),
                            b.map_or(0., accessories::Sparkle::alpha),
                        ],
                    });
                    self.sparkles.push(source.clone());
                }
            }
            for frame in from {
                let future = to
                    .iter()
                    .find(|next| next.pass == frame.pass && next.bones.len() == frame.bones.len())
                    .unwrap_or(&frame);
                let key = if state.source.attachment.is_none() {
                    let key = Key::Independent(independent);
                    independent += 1;
                    key
                } else {
                    Key::Accessory(
                        state.source.owner,
                        frame.pass,
                        matches!(
                            state.source.style,
                            resonance_game::battle::encounter::AccessoryStyle::Ordinary
                        ),
                    )
                };
                let uv = frame
                    .uv
                    .zip(future.uv)
                    .filter(|(a, b)| a.0 == b.0)
                    .map(|(a, b)| (a.0, a.1, b.1));
                self.attachments.push(Accessory {
                    tint: (frame.tint, future.tint),
                    uv,
                    source: state.source.clone(),
                    bones: Matrices::new(&frame.bones, &future.bones),
                });
                self.accessories.push((state.source.resource, key, frame));
            }
        }
        self.key = Some((input.revision, input.next.is_some()));
        Ok(())
    }
}
fn color(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}
fn fade_alpha(
    alpha: u8,
    minimum: u8,
    step: u8,
    elapsed: u64,
    frame: &BattleFrame,
    point: Vec3,
) -> Result<u8> {
    let camera = frame.camera.context("battle scenery has no camera")?;
    let target = if scenery_in_foreground(
        Vec2::new(camera.eye[0], camera.eye[2]),
        Vec2::new(camera.focus[0], camera.focus[2]),
        Vec2::new(point.x, point.z),
    ) {
        minimum
    } else {
        u8::MAX
    };
    let step = (u64::from(step) * elapsed).min(255) as u8;
    Ok(if alpha > target {
        alpha.saturating_sub(step).max(target)
    } else {
        alpha.saturating_add(step).min(target)
    })
}

/// Attachments must make the same cut decision as their rendered owner.
pub(crate) fn continuous_model(
    a: &resonance_battle::ModelFrame,
    b: &resonance_battle::ModelFrame,
) -> bool {
    a.actor == b.actor
        && a.resource == b.resource
        && a.visible == b.visible
        && a.bones.len() == b.bones.len()
        && Vec3::from_array(a.world[3][..3].try_into().unwrap())
            .distance_squared(Vec3::from_array(b.world[3][..3].try_into().unwrap()))
            <= 128. * 128.
}
