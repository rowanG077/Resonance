use super::{AnimatedPose, Clock, ModelFrame, ModelMaterial, Playback, secondary};
use crate::ActorId;
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::{Matrix, Motion, Skeleton, multiply},
    secondary_motion::{Chain, Simulation},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub enum WeaponPlayback {
    Rigid,
    /// An independent authored loop, unaffected by the owner's action changes.
    Local(Playback),
    Owner {
        offset: u16,
        fallback: u16,
        initial: Playback,
    },
    /// A body component follows action changes with an exact clip offset.
    Body {
        offset: u16,
        initial: Playback,
    },
}

#[derive(Debug, Clone)]
pub struct WeaponLayerDefinition {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub playback: WeaponPlayback,
    pub secondary_motion: Vec<Chain>,
    pub texture_animation: Option<WeaponTextureAnimation>,
}

#[derive(Debug, Clone)]
pub enum WeaponTextureAnimation {
    /// An independent offset, measured in thousandths of one UV repeat.
    Scroll {
        texture: usize,
        initial: [i16; 2],
        step: [i16; 2],
    },
    Atlas {
        texture: usize,
        rows: u8,
        ticks_per_frame: u16,
        order: Vec<u8>,
    },
}

const UV_REPEAT: i16 = 1000;

#[derive(Debug, Clone)]
pub struct WeaponDefinition {
    pub slot: u8,
    pub initially_visible: bool,
    pub attachment: Option<u16>,
    pub effect_interval: Option<u16>,
    pub(crate) layers: Vec<WeaponLayerDefinition>,
    /// Rope segments use the primary layer's bones.
    pub(crate) links: Vec<[u16; 2]>,
}

impl WeaponDefinition {
    /// Admit layer relationships once, after the loader validates skeletons and motions.
    /// Attachment bones and slots are checked when binding to a body.
    pub fn new(
        slot: u8,
        attachment: u16,
        layers: Vec<WeaponLayerDefinition>,
        links: Vec<[u16; 2]>,
    ) -> Result<Self> {
        let definition = Self {
            slot,
            initially_visible: true,
            attachment: Some(attachment),
            effect_interval: None,
            layers,
            links,
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn primary_resource(&self) -> u32 {
        self.layers[0].resource
    }

    pub fn bone_count(&self) -> usize {
        self.layers[0].skeleton.bones.len()
    }

    pub fn has_carried_motion(&self, clip: u16) -> bool {
        !matches!(
            self.layers[0].playback,
            WeaponPlayback::Owner { .. } | WeaponPlayback::Body { .. }
        ) && self.layers[0].motions.contains_key(&clip)
    }

    pub(crate) fn is_body(&self) -> bool {
        self.layers
            .first()
            .is_some_and(|layer| matches!(layer.playback, WeaponPlayback::Body { .. }))
    }

    #[cfg(feature = "test-support")]
    pub fn body_layers(&self) -> Option<&[WeaponLayerDefinition]> {
        self.is_body().then_some(&self.layers)
    }

    /// Extra body artwork has no equipment slot and may share the actor root.
    pub fn body(attachment: Option<u16>, layers: Vec<WeaponLayerDefinition>) -> Result<Self> {
        ensure!(
            layers
                .iter()
                .all(|layer| matches!(layer.playback, WeaponPlayback::Body { .. })),
            "body component requires mapped playback"
        );
        let definition = Self {
            slot: u8::MAX,
            initially_visible: true,
            attachment,
            effect_interval: None,
            layers,
            links: vec![],
        };
        definition.validate()?;
        Ok(definition)
    }

    fn validate(&self) -> Result<()> {
        let primary = self.layers.first().context("weapon has no visual layers")?;
        ensure!(
            self.links
                .iter()
                .flatten()
                .all(|&bone| usize::from(bone) < primary.skeleton.bones.len()),
            "invalid weapon rope binding"
        );
        let mut resources = std::collections::BTreeSet::new();
        for layer in &self.layers {
            ensure!(
                resources.insert(layer.resource),
                "duplicate weapon layer resource"
            );
            for chain in &layer.secondary_motion {
                chain.validate(layer.skeleton.bones.len())?;
            }
            match &layer.texture_animation {
                Some(WeaponTextureAnimation::Scroll { initial, step, .. }) => ensure!(
                    initial
                        .iter()
                        .all(|&value| value > -UV_REPEAT && value < UV_REPEAT)
                        && step.iter().all(|&value| (-128..=127).contains(&value)),
                    "invalid weapon texture scroll"
                ),
                Some(WeaponTextureAnimation::Atlas {
                    rows,
                    ticks_per_frame,
                    order,
                    ..
                }) => ensure!(
                    *rows > 0
                        && *ticks_per_frame > 0
                        && !order.is_empty()
                        && order.iter().all(|row| row < rows),
                    "invalid weapon texture atlas"
                ),
                None => {}
            }
            let initial = match layer.playback {
                WeaponPlayback::Rigid => continue,
                WeaponPlayback::Local(initial) | WeaponPlayback::Body { initial, .. } => initial,
                WeaponPlayback::Owner {
                    fallback, initial, ..
                } => {
                    ensure!(
                        layer.motions.contains_key(&fallback),
                        "missing fallback weapon motion"
                    );
                    initial
                }
            };
            let motion = layer
                .motions
                .get(&initial.clip)
                .context("missing initial weapon motion")?;
            Clock::new(initial, motion.duration_frames, 0)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeaponFrame {
    pub owner: ActorId,
    pub slot: u8,
    pub visible: bool,
    pub tint: [u8; 4],
    pub material: ModelMaterial,
    pub resource: u32,
    pub clip: Option<u16>,
    pub frame: f32,
    pub world: Matrix,
    pub bones: Arc<Vec<Matrix>>,
    pub links: Vec<[u16; 2]>,
    pub texture_scroll: Option<(usize, [f32; 2])>,
}

#[derive(Debug, Clone)]
struct Layer {
    animation: Option<(u16, AnimatedPose)>,
    action_pose: bool,
    secondary: Vec<Simulation>,
    shown: WeaponFrame,
    scroll: [i16; 2],
}

#[derive(Debug, Clone)]
pub(crate) struct Weapon {
    pub(super) definition: Arc<WeaponDefinition>,
    layers: Vec<Layer>,
    detached: Option<Matrix>,
    pub(super) dropped: bool,
    texture_age: u64,
    pub visible: bool,
    /// Unequipped action weapons retain their last geometry until enabled again.
    pub enabled: bool,
}

pub(super) struct PreparedPlay(Vec<Option<(u16, Clock)>>);

#[cfg(test)]
#[path = "item_preflight_tests.rs"]
mod item_preflight_tests;

impl Weapon {
    pub(super) fn play_carried_pose(&mut self, clip: u16, rate: f32, repeat: bool) -> Result<()> {
        let definition = &self.definition.layers[0];
        ensure!(
            !matches!(
                definition.playback,
                WeaponPlayback::Owner { .. } | WeaponPlayback::Body { .. }
            ),
            "carried pose requires an independent track"
        );
        let motion = definition
            .motions
            .get(&clip)
            .context("unprepared carried motion")?;
        let playback = Playback {
            clip,
            frame: 0.,
            rate,
            repeat,
        };
        let clock = Clock::new(playback, motion.duration_frames, 0)?;
        if let Some((current, animation)) = &mut self.layers[0].animation {
            animation.start(clock);
            *current = clip;
        } else {
            let mut animation = AnimatedPose::new(&definition.skeleton, motion, playback)?;
            animation.pending_sample = true;
            self.layers[0].animation = Some((clip, animation));
        }
        self.layers[0].action_pose = true;
        Ok(())
    }

    #[cfg(feature = "test-support")]
    pub(super) fn restore_fixture_clock(
        &mut self,
        clip: u16,
        pose: super::Pose,
        elapsed_updates: u32,
    ) -> Result<()> {
        ensure!(
            self.definition.is_body(),
            "fixture clock requires a body component"
        );
        for (layer, definition) in self.layers.iter_mut().zip(&self.definition.layers) {
            let motion = definition
                .motions
                .get(&clip)
                .context("unprepared secondary ready clip")?;
            let mut clock = Clock::new(
                Playback {
                    clip,
                    frame: pose.frame,
                    rate: pose.rate,
                    repeat: pose.repeat,
                },
                motion.duration_frames,
                0,
            )?;
            ensure!(
                pose.loop_start.is_finite() && (0.0..=clock.end).contains(&pose.loop_start),
                "invalid secondary ready loop origin"
            );
            clock.loop_start = pose.loop_start;
            for _ in 0..elapsed_updates {
                clock.step();
            }
            let (current, animation) = layer
                .animation
                .as_mut()
                .context("missing secondary track")?;
            *current = clip;
            animation.start(clock);
            animation.sample(&definition.skeleton, motion, 1., false)?;
            animation.pending_sample = true;
        }
        Ok(())
    }

    pub(super) fn set_animation_rate(&mut self, rate: f32) -> Result<()> {
        ensure!(
            rate.is_finite() && rate > 0.,
            "invalid carried animation rate"
        );
        ensure!(
            matches!(self.definition.layers[0].playback, WeaponPlayback::Local(_)),
            "carried animation requires an independent track"
        );
        let (_, animation) = self.layers[0]
            .animation
            .as_mut()
            .context("unprepared carried animation")?;
        animation.clock.rate = rate;
        Ok(())
    }

    pub(super) fn slot(&self) -> u8 {
        self.definition.slot
    }
    pub(super) fn frames(&self) -> impl Iterator<Item = &WeaponFrame> {
        self.layers.iter().map(|layer| &layer.shown)
    }
    pub(super) fn frames_mut(&mut self) -> impl Iterator<Item = &mut WeaponFrame> {
        self.layers.iter_mut().map(|layer| &mut layer.shown)
    }

    pub(super) fn body_clocks(&mut self) -> impl Iterator<Item = &mut Clock> {
        self.layers
            .iter_mut()
            .zip(&self.definition.layers)
            .filter(|(_, definition)| matches!(definition.playback, WeaponPlayback::Body { .. }))
            .filter_map(|(layer, _)| {
                layer
                    .animation
                    .as_mut()
                    .map(|(_, animation)| &mut animation.clock)
            })
    }

    pub(super) fn advance_body_pose(&mut self) -> Result<()> {
        for (layer, definition) in self.layers.iter_mut().zip(&self.definition.layers) {
            if matches!(definition.playback, WeaponPlayback::Body { .. })
                && let Some((clip, animation)) = &mut layer.animation
            {
                animation.advance(&definition.skeleton, &definition.motions[clip])?;
            }
        }
        Ok(())
    }

    pub fn new(definition: Arc<WeaponDefinition>, owner: &ModelFrame) -> Result<Self> {
        ensure!(
            definition
                .attachment
                .is_none_or(|bone| usize::from(bone) < owner.bones.len()),
            "invalid weapon attachment bone"
        );
        let mut layers = Vec::new();
        for (index, layer) in definition.layers.iter().enumerate() {
            let animation = match layer.playback {
                WeaponPlayback::Rigid => None,
                WeaponPlayback::Local(initial)
                | WeaponPlayback::Owner { initial, .. }
                | WeaponPlayback::Body { initial, .. } => Some((
                    initial.clip,
                    AnimatedPose::new(&layer.skeleton, &layer.motions[&initial.clip], initial)?,
                )),
            };
            let bones = match &animation {
                Some((_, pose)) => pose.pose(&layer.skeleton, [false; 3])?.0.global,
                None => layer.skeleton.bind_pose()?.global,
            };
            layers.push(Layer {
                action_pose: false,
                secondary: vec![Simulation::default(); layer.secondary_motion.len()],
                scroll: match &layer.texture_animation {
                    Some(WeaponTextureAnimation::Scroll { initial, .. }) => *initial,
                    _ => [0; 2],
                },
                shown: WeaponFrame {
                    owner: owner.actor,
                    slot: definition.slot,
                    visible: owner.visible,
                    tint: owner.tint,
                    material: owner.material,
                    resource: layer.resource,
                    clip: animation.as_ref().map(|(clip, _)| *clip),
                    frame: animation.as_ref().map_or(0., |(_, pose)| pose.clock.frame),
                    world: definition.attachment.map_or(owner.world, |bone| {
                        multiply(owner.world, owner.bones[usize::from(bone)])
                    }),
                    bones: Arc::new(bones),
                    links: if index == 0 {
                        definition.links.clone()
                    } else {
                        vec![]
                    },
                    texture_scroll: None,
                },
                animation,
            });
        }
        let mut weapon = Self {
            visible: definition.initially_visible,
            definition,
            layers,
            detached: None,
            dropped: false,
            texture_age: 0,
            enabled: true,
        };
        weapon.step(owner, false)?;
        Ok(weapon)
    }

    pub(super) fn rebind(
        definition: Arc<WeaponDefinition>,
        owner: &ModelFrame,
        previous: Option<&Self>,
        body: Playback,
        duration: f32,
    ) -> Result<Self> {
        if let Some(previous) = previous
            && Arc::ptr_eq(&definition, &previous.definition)
        {
            let mut next = previous.clone();
            next.step(owner, false)?;
            return Ok(next);
        }
        let mut next = Self::new(definition, owner)?;
        next.apply_play(next.prepare_play(body, duration, 0)?);
        for (layer, definition) in next.layers.iter_mut().zip(&next.definition.layers) {
            if let Some((clip, animation)) = &mut layer.animation {
                animation.sample(&definition.skeleton, &definition.motions[clip], 1., false)?;
            }
        }
        if let Some(previous) = previous {
            next.detached = previous.detached;
            next.dropped = previous.dropped;
        }
        next.step(owner, false)?;
        if let Some(previous) = previous {
            next.visible = previous.visible;
            next.enabled = previous.enabled;
            for frame in next.frames_mut() {
                frame.visible &= previous.visible && previous.enabled;
            }
        }
        Ok(next)
    }

    #[cfg(test)]
    pub fn play(&mut self, body: Playback, duration: f32, blend: u8) -> Result<()> {
        let prepared = self.prepare_play(body, duration, blend)?;
        self.apply_play(prepared);
        Ok(())
    }

    pub(super) fn prepare_play(
        &self,
        body: Playback,
        duration: f32,
        blend: u8,
    ) -> Result<PreparedPlay> {
        self.definition
            .layers
            .iter()
            .map(|layer| {
                if let WeaponPlayback::Body { offset, .. } = layer.playback {
                    let clip = body
                        .clip
                        .checked_add(offset)
                        .context("body component clip overflow")?;
                    let motion = layer
                        .motions
                        .get(&clip)
                        .context("missing body component action motion")?;
                    let clock =
                        Clock::new(Playback { clip, ..body }, motion.duration_frames, blend)?;
                    return Ok(Some((clip, clock)));
                }
                let WeaponPlayback::Owner {
                    offset, fallback, ..
                } = layer.playback
                else {
                    return Ok(None);
                };
                let mapped = body
                    .clip
                    .checked_add(offset)
                    .filter(|clip| layer.motions.contains_key(clip));
                let clip = mapped.unwrap_or(fallback);
                let motion = &layer.motions[&clip];
                // The owner validated this playback before staging its attachments.
                let frame = if mapped.is_some() {
                    body.frame / duration * motion.duration_frames
                } else {
                    0.
                };
                let mut clock = Clock::new(
                    Playback {
                        clip,
                        frame,
                        rate: body.rate,
                        repeat: body.repeat,
                    },
                    motion.duration_frames,
                    blend,
                )?;
                clock.loop_start = 0.;
                Ok(Some((clip, clock)))
            })
            .collect::<Result<Vec<_>>>()
            .map(PreparedPlay)
    }

    pub(super) fn apply_play(&mut self, prepared: PreparedPlay) {
        for (layer, prepared) in self.layers.iter_mut().zip(prepared.0) {
            if let (Some((current, animation)), Some((clip, clock))) =
                (&mut layer.animation, prepared)
            {
                animation.start(clock);
                *current = clip;
            }
        }
    }

    pub fn step(&mut self, owner: &ModelFrame, advance: bool) -> Result<()> {
        self.step_with_clocks(owner, advance, advance)
    }

    pub(super) fn step_with_clocks(
        &mut self,
        owner: &ModelFrame,
        advance: bool,
        carried_advance: bool,
    ) -> Result<()> {
        if !self.enabled {
            for layer in &mut self.layers {
                layer.shown.visible = false;
            }
            return Ok(());
        }
        self.texture_age += u64::from(advance);
        let world = self.world_at(owner, owner.world);
        let (_, rotation, _) =
            glam::Mat4::from_cols_array_2d(&world).to_scale_rotation_translation();
        for (layer, definition) in self.layers.iter_mut().zip(&self.definition.layers) {
            layer.shown.texture_scroll = match &definition.texture_animation {
                Some(WeaponTextureAnimation::Scroll { texture, step, .. }) => {
                    if advance {
                        for (value, step) in layer.scroll.iter_mut().zip(step) {
                            *value = (*value + step) % UV_REPEAT;
                        }
                    }
                    Some((
                        *texture,
                        layer
                            .scroll
                            .map(|value| f32::from(value) / f32::from(UV_REPEAT)),
                    ))
                }
                Some(WeaponTextureAnimation::Atlas {
                    texture,
                    rows,
                    ticks_per_frame,
                    order,
                }) => {
                    let frame = self.texture_age / u64::from(*ticks_per_frame);
                    let row = order[(frame % order.len() as u64) as usize];
                    Some((*texture, [0., f32::from(row) / f32::from(*rows)]))
                }
                None => None,
            };
            let mut bones = if let Some((clip, animation)) = &mut layer.animation {
                let motion = &definition.motions[clip];
                let advance_pose = if layer.action_pose {
                    carried_advance
                } else {
                    advance
                };
                if advance_pose {
                    animation.advance(&definition.skeleton, motion)?;
                } else {
                    animation.sample_pending(&definition.skeleton, motion)?;
                }
                layer.shown.clip = Some(*clip);
                layer.shown.frame = animation.clock.frame;
                animation.pose(&definition.skeleton, [false; 3])?.0.global
            } else {
                definition.skeleton.bind_pose()?.global
            };
            secondary::apply(
                &definition.secondary_motion,
                &mut layer.secondary,
                &mut bones,
                secondary::Placement {
                    world,
                    rotation,
                    acceleration: glam::Vec3::ZERO,
                },
                advance,
            )?;
            layer.shown.bones = Arc::new(bones);
            layer.shown.world = world;
            layer.shown.visible = owner.visible && self.visible;
            layer.shown.tint = owner.tint;
            layer.shown.material = owner.material;
        }
        Ok(())
    }

    pub(super) fn world_at(&self, owner: &ModelFrame, placement: Matrix) -> Matrix {
        self.detached.unwrap_or_else(|| {
            self.definition.attachment.map_or(placement, |bone| {
                multiply(placement, owner.bones[usize::from(bone)])
            })
        })
    }
    pub fn detach(&mut self, world: Option<Matrix>) {
        self.detached = world;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::{Bone, Transform, TransformChannels};

    fn owner() -> ModelFrame {
        ModelFrame {
            actor: ActorId(2),
            depth_write: true,
            visible: true,
            tint: [128; 4],
            material: ModelMaterial::Normal,

            texture_layers: [0; 4],
            light: None,
            shadow: None,
            resource: 7,
            clip: 0,
            frame: 9.,
            blend_weight: 1.,
            root_translation: [0.; 3],
            world: Transform {
                translation: [100., 200., 300.],
                ..Default::default()
            }
            .matrix(),
            particle_world: Transform {
                translation: [100., 200., 300.],
                ..Default::default()
            }
            .matrix(),
            bones: Arc::new(vec![
                Transform {
                    translation: [10., 20., 30.],
                    ..Default::default()
                }
                .matrix(),
            ]),
        }
    }

    fn definition() -> WeaponDefinition {
        WeaponDefinition {
            slot: 0,
            attachment: Some(0),
            effect_interval: None,
            initially_visible: true,
            layers: vec![WeaponLayerDefinition {
                resource: 70,
                skeleton: Skeleton {
                    bones: vec![Bone {
                        name: "weapon".into(),
                        parent: None,
                        bind_channels: TransformChannels(8),
                        bind: Transform {
                            translation: [1., 2., 3.],
                            ..Default::default()
                        },
                    }],
                },
                motions: [(60, 6.), (90, 20.)]
                    .into_iter()
                    .map(|(clip, duration_frames)| {
                        (
                            clip,
                            Motion {
                                duration_frames,
                                tracks: vec![],
                            },
                        )
                    })
                    .collect(),
                playback: WeaponPlayback::Owner {
                    offset: 60,
                    fallback: 60,
                    initial: Playback {
                        clip: 60,
                        frame: 0.,
                        rate: 1.,
                        repeat: true,
                    },
                },
                secondary_motion: vec![],
                texture_animation: None,
            }],
            links: vec![],
        }
    }

    #[test]
    fn initially_hidden_attachment_stays_hidden_until_explicitly_shown() -> Result<()> {
        let owner = owner();
        let visible = Weapon::new(Arc::new(definition()), &owner)?;
        assert!(visible.frames().all(|frame| frame.visible));
        let mut definition = definition();
        definition.initially_visible = false;
        let mut hidden = Weapon::new(Arc::new(definition), &owner)?;
        assert!(hidden.frames().all(|frame| !frame.visible));
        hidden.step(&owner, true)?;
        assert!(hidden.frames().all(|frame| !frame.visible));
        hidden.visible = true;
        hidden.step(&owner, false)?;
        assert!(hidden.frames().all(|frame| frame.visible));
        Ok(())
    }

    #[test]
    fn texture_scroll_keeps_signed_remainders_and_holds_without_restarting_on_body_motion()
    -> Result<()> {
        let mut definition = definition();
        definition.layers[0].texture_animation = Some(WeaponTextureAnimation::Scroll {
            texture: 1,
            initial: [0, 5],
            step: [0, -7],
        });
        let mut independent = definition.layers[0].clone();
        independent.resource = 71;
        independent.texture_animation = Some(WeaponTextureAnimation::Scroll {
            texture: 0,
            initial: [0, 995],
            step: [0, 11],
        });
        definition.layers.push(independent);
        let mut horizontal = definition.layers[0].clone();
        horizontal.resource = 72;
        horizontal.texture_animation = Some(WeaponTextureAnimation::Scroll {
            texture: 0,
            initial: [995, 0],
            step: [7, 0],
        });
        definition.layers.push(horizontal);
        let definition = Arc::new(definition);
        let owner = owner();
        let mut weapon = Weapon::new(definition.clone(), &owner)?;
        for _ in 0..11 {
            weapon.step(&owner, true)?;
        }
        let offsets = |weapon: &Weapon| {
            weapon
                .frames()
                .map(|frame| frame.texture_scroll.unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            offsets(&weapon),
            vec![(1, [0., -0.072]), (0, [0., 0.116]), (0, [0.072, 0.])]
        );
        let held = offsets(&weapon);
        weapon.step(&owner, false)?;
        weapon.play(
            Playback {
                clip: 30,
                frame: 0.,
                rate: 1.,
                repeat: false,
            },
            20.,
            0,
        )?;
        weapon.step(&owner, false)?;
        assert_eq!(offsets(&weapon), held);
        let rebound = Weapon::rebind(
            definition,
            &owner,
            Some(&weapon),
            Playback {
                clip: 0,
                frame: 0.,
                rate: 1.,
                repeat: true,
            },
            6.,
        )?;
        assert_eq!(offsets(&rebound), held);
        Ok(())
    }

    #[test]
    fn texture_atlases_keep_row_durations_sequences_and_phase_across_holds() -> Result<()> {
        let mut definition = definition();
        let primary = definition.layers[0].clone();
        definition.layers = [
            (4, 9, vec![0, 1, 2, 3, 2, 1]),
            (8, 5, vec![0, 0, 0, 0, 4, 5, 6, 7, 7, 7, 6, 5, 0, 0, 0, 0]),
            (32, 3, (0..32).collect()),
        ]
        .into_iter()
        .enumerate()
        .map(|(texture, (rows, ticks_per_frame, order))| {
            let mut layer = primary.clone();
            layer.resource += texture as u32;
            layer.texture_animation = Some(WeaponTextureAnimation::Atlas {
                texture,
                rows,
                ticks_per_frame,
                order,
            });
            layer
        })
        .collect();
        definition.validate()?;
        let definition = Arc::new(definition);
        let owner = owner();
        let mut weapon = Weapon::new(definition.clone(), &owner)?;
        // Boundary rows from the retained material traces, aligned to local age.
        let samples = [
            (0, [0, 0, 0]),
            (2, [0, 0, 0]),
            (3, [0, 0, 1]),
            (8, [0, 0, 2]),
            (9, [1, 0, 3]),
            (18, [2, 0, 6]),
            (20, [2, 4, 6]),
            (27, [3, 5, 9]),
            (35, [3, 7, 11]),
            (36, [2, 7, 12]),
            (49, [1, 7, 16]),
            (50, [1, 6, 16]),
            (54, [0, 6, 18]),
            (55, [0, 5, 18]),
            (60, [0, 0, 20]),
            (93, [2, 0, 31]),
            (96, [2, 0, 0]),
            (100, [1, 4, 1]),
        ];
        let mut age = 0;
        for (tick, rows) in samples {
            while age < tick {
                weapon.step(&owner, true)?;
                age += 1;
            }
            weapon.step(&owner, false)?;
            for (texture, ((frame, row), total)) in
                weapon.frames().zip(rows).zip([4., 8., 32.]).enumerate()
            {
                assert_eq!(
                    frame.texture_scroll,
                    Some((texture, [0., row as f32 / total])),
                    "age {age}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn local_weapon_layers_loop_independently_and_hold_with_the_owner() -> Result<()> {
        use resonance_content::animation::{Track, VectorCurve, VectorInterpolation};
        let mut definition = definition();
        let primary = &mut definition.layers[0];
        primary.playback = WeaponPlayback::Local(Playback {
            clip: 0,
            frame: 0.,
            rate: 1.,
            repeat: true,
        });
        primary.motions = [(
            0,
            Motion {
                duration_frames: 4.,
                tracks: vec![Track {
                    bone: 0,
                    bind_channels: TransformChannels(8),
                    period_frames: 4.,
                    times: vec![0., 4.],
                    translation: Some(VectorCurve {
                        interpolation: VectorInterpolation::Linear,
                        values: vec![[1., 2., 3.], [9., 2., 3.]],
                        incoming: vec![],
                        outgoing: vec![],
                        ease: vec![],
                    }),
                    scale: None,
                    rotation: None,
                    euler_degrees: None,
                    matrices: None,
                }],
            },
        )]
        .into();
        let mut decoration = primary.clone();
        decoration.resource += 1;
        decoration.playback = WeaponPlayback::Local(Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        });
        definition.layers.push(decoration);
        let owner = owner();
        let mut weapon = Weapon::new(Arc::new(definition), &owner)?;
        let initial: Vec<_> = weapon.frames().cloned().collect();
        weapon.step(&owner, true)?;
        let moving: Vec<_> = weapon.frames().cloned().collect();
        assert!(moving.iter().zip(&initial).all(|(a, b)| a.bones != b.bones));
        assert_ne!(moving[0].bones, moving[1].bones);
        weapon.play(
            Playback {
                clip: 30,
                frame: 7.,
                rate: 0.,
                repeat: false,
            },
            10.,
            0,
        )?;
        weapon.step(&owner, false)?;
        assert_eq!(weapon.frames().cloned().collect::<Vec<_>>(), moving);
        let mut looped = [false; 2];
        let mut previous = [moving[0].frame, moving[1].frame];
        for _ in 0..16 {
            weapon.step(&owner, true)?;
            for (index, frame) in weapon.frames().enumerate() {
                looped[index] |= frame.frame < previous[index];
                previous[index] = frame.frame;
                assert!((0. ..=4.).contains(&frame.frame));
            }
        }
        assert_eq!(looped, [true; 2]);
        let detached = Transform {
            translation: [-20., 5., 40.],
            ..Default::default()
        }
        .matrix();
        weapon.detach(Some(detached));
        weapon.step(&owner, false)?;
        assert!(weapon.frames().all(|frame| frame.world == detached));
        weapon.enabled = false;
        let hidden: Vec<_> = weapon.frames().cloned().collect();
        let mut moved_owner = owner.clone();
        moved_owner.world[3][0] += 100.;
        weapon.detach(None);
        weapon.step(&moved_owner, true)?;
        assert!(weapon.frames().all(|frame| !frame.visible));
        for (frame, previous) in weapon.frames().zip(&hidden) {
            assert_eq!(frame.world, previous.world);
            assert_eq!(frame.bones, previous.bones);
            assert_eq!(frame.frame, previous.frame);
        }
        weapon.enabled = true;
        weapon.step(&moved_owner, true)?;
        assert!(weapon.frames().all(|frame| frame.visible));
        assert_ne!(weapon.frames().next().unwrap().world, hidden[0].world);
        Ok(())
    }

    #[test]
    fn model_components_validate_clips_and_keep_their_anchor() -> Result<()> {
        let mut missing = definition();
        missing.layers[0].motions.remove(&60);
        assert!(missing.validate().is_err());
        let mut rigid = definition();
        rigid.layers[0].playback = WeaponPlayback::Rigid;
        let mut owner = owner();
        let mut weapon = Weapon::new(Arc::new(rigid), &owner)?;
        owner.bones = Arc::new(vec![
            Transform {
                translation: [-10., 0., 0.],
                ..Default::default()
            }
            .matrix(),
        ]);
        weapon.step(&owner, false)?;
        assert_eq!(weapon.layers[0].shown.clip, None);
        assert_eq!(
            weapon.layers[0].shown.world,
            weapon.world_at(&owner, owner.world)
        );
        let mut layers = definition().layers;
        layers[0].playback = WeaponPlayback::Body {
            offset: 60,
            initial: Playback {
                clip: 60,
                frame: 1.5,
                rate: 0.5,
                repeat: true,
            },
        };
        let mut component = Weapon::new(Arc::new(WeaponDefinition::body(None, layers)?), &owner)?;
        let action = Playback {
            clip: 30,
            frame: 3.25,
            rate: 0.5,
            repeat: false,
        };
        component.play(action, 10., 0)?;
        component.step(&owner, false)?;
        assert_eq!(component.layers[0].shown.clip, Some(90));
        assert_eq!(component.layers[0].shown.frame, 3.25);
        assert_eq!(component.layers[0].shown.world, owner.world);
        let held = component.layers[0].shown.clone();
        assert!(
            component
                .play(Playback { clip: 31, ..action }, 10., 0)
                .is_err()
        );
        component.step(&owner, false)?;
        assert_eq!(component.layers[0].shown, held);
        component.step(&owner, true)?;
        assert_eq!(component.layers[0].shown.frame, 3.75);
        Ok(())
    }

    #[test]
    fn detached_weapon_uses_flight_placement_and_returns_to_its_owner() -> Result<()> {
        let mut owner = owner();
        let mut weapon = Weapon::new(Arc::new(definition()), &owner)?;
        let carried = weapon.layers[0].shown.clone();
        let flight = Transform {
            translation: [-40., 5.1, 80.],
            ..Default::default()
        }
        .matrix();
        weapon.detach(Some(flight));
        assert_eq!(weapon.layers[0].shown, carried);

        // A held animation still draws the preceding flight callback's world.
        weapon.step(&owner, false)?;
        assert_eq!(weapon.layers[0].shown.world, flight);
        assert_eq!(weapon.layers[0].shown.frame, carried.frame);
        assert_eq!(weapon.layers[0].shown.bones, carried.bones);

        let placement = owner.world;
        owner.world[3][0] += 20.;
        assert_eq!(weapon.world_at(&owner, placement), flight);
        weapon.detach(None);
        assert_eq!(weapon.layers[0].shown.world, flight);
        assert_eq!(weapon.world_at(&owner, placement), carried.world);
        weapon.step(&owner, false)?;
        assert_eq!(
            weapon.layers[0].shown.world,
            weapon.world_at(&owner, owner.world)
        );
        Ok(())
    }

    #[test]
    fn rebinding_keeps_unchanged_animation_and_initializes_changed_definitions() -> Result<()> {
        use resonance_content::animation::{Track, VectorCurve, VectorInterpolation};

        let owner = owner();
        let curve = |values| VectorCurve {
            interpolation: VectorInterpolation::Linear,
            values,
            incoming: vec![],
            outgoing: vec![],
            ease: vec![],
        };
        let mut definition = definition();
        definition.layers[0].motions.get_mut(&90).unwrap().tracks = vec![Track {
            bone: 0,
            bind_channels: TransformChannels(8),
            period_frames: 20.,
            times: vec![0., 20.],
            translation: Some(curve(vec![[1., 2., 3.], [21., 2., 3.]])),
            scale: Some(curve(vec![[2., 3., 4.]; 2])),
            rotation: None,
            euler_degrees: None,
            matrices: None,
        }];
        let definition = Arc::new(definition);
        let mut previous = Weapon::new(Arc::clone(&definition), &owner)?;
        let body = Playback {
            clip: 30,
            frame: 2.,
            rate: 0.25,
            repeat: true,
        };
        previous.play(body, 10., 0)?;
        previous.detach(Some(Transform::default().matrix()));
        previous.visible = false;
        previous.step(&owner, true)?;
        assert_eq!(previous.layers[0].shown.frame, 4.);
        let mut rebound =
            Weapon::rebind(Arc::clone(&definition), &owner, Some(&previous), body, 10.)?;
        assert_eq!(rebound.layers[0].shown, previous.layers[0].shown);
        previous.step(&owner, true)?;
        rebound.step(&owner, true)?;
        assert_eq!(rebound.layers[0].shown, previous.layers[0].shown);
        assert_eq!(rebound.layers[0].shown.frame, 4.25);

        // A changed owner-linked weapon follows the body's current action,
        // even though its entry recipe starts on idle clip 60.
        let mut rebound = Weapon::rebind(
            Arc::new((*definition).clone()),
            &owner,
            Some(&previous),
            Playback { frame: 4., ..body },
            10.,
        )?;
        assert_eq!(
            (rebound.layers[0].shown.clip, rebound.layers[0].shown.frame),
            (Some(90), 8.)
        );
        rebound.step(&owner, true)?;
        assert_eq!(rebound.layers[0].shown.frame, 8.25);

        let mut changed = (*definition).clone();
        let layer = &mut changed.layers[0];
        layer.playback = WeaponPlayback::Local(Playback {
            clip: 90,
            frame: 1.,
            rate: 0.5,
            repeat: true,
        });
        let track = &mut layer.motions.get_mut(&90).unwrap().tracks[0];
        track.translation = Some(curve(vec![[31., 2., 3.], [71., 2., 3.]]));
        track.scale = None;
        changed.validate()?;
        let mut rebound = Weapon::rebind(Arc::new(changed), &owner, Some(&previous), body, 10.)?;
        for (frame, x) in [(1., 33.), (1.5, 34.)] {
            let shown = &rebound.layers[0].shown;
            assert_eq!((shown.clip, shown.frame), (Some(90), frame));
            assert_eq!(shown.world, previous.layers[0].shown.world);
            assert!(!shown.visible);
            assert_eq!(
                shown.bones[0],
                Transform {
                    translation: [x, 2., 3.],
                    ..Default::default()
                }
                .matrix()
            );
            rebound.step(&owner, true)?;
        }
        Ok(())
    }
}
