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
}

#[derive(Debug, Clone)]
pub struct WeaponLayerDefinition {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub playback: WeaponPlayback,
    pub secondary_motion: Vec<Chain>,
}

#[derive(Debug, Clone)]
pub struct WeaponDefinition {
    pub slot: u8,
    pub attachment: u16,
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
            attachment,
            layers,
            links,
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn primary_resource(&self) -> u32 {
        self.layers[0].resource
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
            let initial = match layer.playback {
                WeaponPlayback::Rigid => {
                    ensure!(layer.motions.is_empty(), "rigid weapon has motions");
                    continue;
                }
                WeaponPlayback::Local(initial) => initial,
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
}

#[derive(Debug, Clone)]
struct Layer {
    animation: Option<(u16, AnimatedPose)>,
    secondary: Vec<Simulation>,
    shown: WeaponFrame,
}

#[derive(Debug, Clone)]
pub(crate) struct Weapon {
    definition: Arc<WeaponDefinition>,
    layers: Vec<Layer>,
    detached: Option<Matrix>,
    pub visible: bool,
}

pub(super) struct PreparedPlay(Vec<Option<(u16, Clock)>>);

#[cfg(test)]
#[path = "item_preflight_tests.rs"]
mod item_preflight_tests;

impl Weapon {
    pub(super) fn slot(&self) -> u8 {
        self.definition.slot
    }
    pub(super) fn frames(&self) -> impl Iterator<Item = &WeaponFrame> {
        self.layers.iter().map(|layer| &layer.shown)
    }
    pub(super) fn frames_mut(&mut self) -> impl Iterator<Item = &mut WeaponFrame> {
        self.layers.iter_mut().map(|layer| &mut layer.shown)
    }

    pub fn new(definition: Arc<WeaponDefinition>, owner: &ModelFrame) -> Result<Self> {
        ensure!(
            usize::from(definition.attachment) < owner.bones.len(),
            "invalid weapon attachment bone"
        );
        let mut layers = Vec::new();
        for (index, layer) in definition.layers.iter().enumerate() {
            let animation = match layer.playback {
                WeaponPlayback::Rigid => None,
                WeaponPlayback::Local(initial) | WeaponPlayback::Owner { initial, .. } => Some((
                    initial.clip,
                    AnimatedPose::new(&layer.skeleton, &layer.motions[&initial.clip], initial)?,
                )),
            };
            let bones = match &animation {
                Some((_, pose)) => pose.pose(&layer.skeleton, [false; 3])?.0.global,
                None => layer.skeleton.bind_pose()?.global,
            };
            layers.push(Layer {
                secondary: vec![Simulation::default(); layer.secondary_motion.len()],
                shown: WeaponFrame {
                    owner: owner.actor,
                    slot: definition.slot,
                    visible: owner.visible,
                    tint: owner.tint,
                    material: owner.material,
                    resource: layer.resource,
                    clip: animation.as_ref().map(|(clip, _)| *clip),
                    frame: animation.as_ref().map_or(0., |(_, pose)| pose.clock.frame),
                    world: multiply(owner.world, owner.bones[usize::from(definition.attachment)]),
                    bones: Arc::new(bones),
                    links: if index == 0 {
                        definition.links.clone()
                    } else {
                        vec![]
                    },
                },
                animation,
            });
        }
        let mut weapon = Self {
            definition,
            layers,
            detached: None,
            visible: true,
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
        if let Some(previous) = previous {
            next.detached = previous.detached;
            next.visible = previous.visible;
        }
        next.step(owner, false)?;
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
        let world = self.detached.unwrap_or_else(|| self.attached_world(owner));
        let (_, rotation, _) =
            glam::Mat4::from_cols_array_2d(&world).to_scale_rotation_translation();
        for (layer, definition) in self.layers.iter_mut().zip(&self.definition.layers) {
            let mut bones = if let Some((clip, animation)) = &mut layer.animation {
                let motion = &definition.motions[clip];
                if advance {
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
                secondary::Placement { world, rotation },
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

    fn attached_world(&self, owner: &ModelFrame) -> Matrix {
        multiply(
            owner.world,
            owner.bones[usize::from(self.definition.attachment)],
        )
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
            attachment: 0,
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
            }],
            links: vec![],
        }
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
        weapon.visible = false;
        weapon.step(&owner, false)?;
        assert!(weapon.frames().all(|frame| !frame.visible));
        Ok(())
    }

    #[test]
    fn weapon_admission_checks_playback_and_rigid_binding_follows_the_body() -> Result<()> {
        let mut missing = definition();
        missing.layers[0].motions.remove(&60);
        assert!(missing.validate().is_err());
        let mut definition = definition();
        definition.layers[0].playback = WeaponPlayback::Rigid;
        assert!(definition.validate().is_err());
        definition.layers[0].motions.clear();
        let mut owner = owner();
        let mut weapon = Weapon::new(Arc::new(definition), &owner)?;
        owner.bones = Arc::new(vec![
            Transform {
                translation: [-10., 0., 0.],
                ..Default::default()
            }
            .matrix(),
        ]);
        weapon.step(&owner, false)?;
        assert_eq!(weapon.layers[0].shown.clip, None);
        assert_eq!(weapon.layers[0].shown.world, weapon.attached_world(&owner));
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

        owner.world[3][0] += 20.;
        weapon.detach(None);
        assert_eq!(weapon.layers[0].shown.world, flight);
        weapon.step(&owner, false)?;
        assert_eq!(weapon.layers[0].shown.world, weapon.attached_world(&owner));
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
        let mut rebound =
            Weapon::rebind(Arc::clone(&definition), &owner, Some(&previous), body, 10.)?;
        assert_eq!(rebound.layers[0].shown, previous.layers[0].shown);
        previous.step(&owner, true)?;
        rebound.step(&owner, true)?;
        assert_eq!(rebound.layers[0].shown, previous.layers[0].shown);
        assert_eq!(rebound.layers[0].shown.frame, 4.5);

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
