//! Carried models use the actor's sampled attachment, with an independent pose
//! clock when the body motion callback mirrors a second bank (2C05C/155CC).
use super::{Anchor, AnimatedPose, Clock, ModelFrame, Playback};
use crate::ActorId;
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Matrix, Motion, Skeleton, multiply, transform_point};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub enum WeaponPlayback {
    Rigid,
    /// A missing mapped clip selects `fallback`. Initial weapon phase belongs
    /// to construction; the body's randomized entry phase is not copied.
    Owner {
        offset: u16,
        fallback: u16,
        initial: Playback,
    },
}

#[derive(Debug, Clone)]
pub struct WeaponDefinition {
    pub slot: u8,
    pub resource: u32,
    pub attachment: u16,
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub playback: WeaponPlayback,
    /// Appended to the body's contact anchors in weapon definition order.
    pub anchors: Vec<Anchor>,
    pub links: Vec<[u16; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeaponFrame {
    pub owner: ActorId,
    pub slot: u8,
    pub visible: bool,
    pub tint: [u8; 4],
    pub resource: u32,
    pub clip: Option<u16>,
    pub frame: f32,
    pub world: Matrix,
    pub bones: Arc<Vec<Matrix>>,
    pub links: Vec<[u16; 2]>,
}

#[derive(Debug, Clone)]
pub(super) struct Weapon {
    definition: Arc<WeaponDefinition>,
    animation: Option<(u16, AnimatedPose)>,
    detached: Option<Matrix>,
    pub visible: bool,
    pub shown: WeaponFrame,
}

impl Weapon {
    pub fn new(definition: Arc<WeaponDefinition>, owner: &ModelFrame) -> Result<Self> {
        definition.skeleton.validate()?;
        ensure!(
            usize::from(definition.attachment) < owner.bones.len()
                && definition.anchors.iter().all(|a| {
                    usize::from(a.bone) < definition.skeleton.bones.len()
                        && a.offset.iter().all(|v| v.is_finite())
                }),
            "invalid weapon attachment or contact binding"
        );
        ensure!(
            definition
                .links
                .iter()
                .flatten()
                .all(|&bone| usize::from(bone) < definition.skeleton.bones.len()),
            "invalid weapon line binding"
        );
        for motion in definition.motions.values() {
            motion.validate(&definition.skeleton)?;
        }
        let animation = match definition.playback {
            WeaponPlayback::Rigid => {
                ensure!(definition.motions.is_empty(), "rigid weapon has motions");
                None
            }
            WeaponPlayback::Owner {
                fallback, initial, ..
            } => {
                ensure!(
                    definition.motions.contains_key(&fallback),
                    "missing fallback weapon motion"
                );
                let motion = definition
                    .motions
                    .get(&initial.clip)
                    .context("missing initial weapon motion")?;
                Some((
                    initial.clip,
                    AnimatedPose::new(&definition.skeleton, motion, initial)?,
                ))
            }
        };
        let bones = match &animation {
            Some((_, pose)) => pose.pose(&definition.skeleton, [false; 3])?.0.global,
            None => definition.skeleton.bind_pose()?.global,
        };
        let shown = WeaponFrame {
            owner: owner.actor,
            slot: definition.slot,
            visible: owner.visible,
            tint: owner.tint,
            resource: definition.resource,
            clip: animation.as_ref().map(|(clip, _)| *clip),
            frame: animation.as_ref().map_or(0., |(_, pose)| pose.clock.frame),
            world: multiply(owner.world, owner.bones[usize::from(definition.attachment)]),
            bones: Arc::new(bones),
            links: definition.links.clone(),
        };
        Ok(Self {
            definition,
            animation,
            detached: None,
            visible: true,
            shown,
        })
    }

    pub fn play(&mut self, body: Playback, blend: u8) -> Result<()> {
        let WeaponPlayback::Owner {
            offset, fallback, ..
        } = self.definition.playback
        else {
            return Ok(());
        };
        // 2C05C uses half a native frame per authored start tick for the child,
        // independently of the body's absolute playback rate and direction.
        let frame = if body.rate == 0. {
            0.
        } else {
            body.frame / body.rate.abs() * 0.5
        };
        let clip = body
            .clip
            .checked_add(offset)
            .filter(|clip| self.definition.motions.contains_key(clip))
            .unwrap_or(fallback);
        let motion = &self.definition.motions[&clip];
        let (current, animation) = self
            .animation
            .as_mut()
            .context("missing owner-linked weapon clock")?;
        ensure!(
            frame.is_finite() && frame >= 0.,
            "invalid owner-linked weapon start"
        );
        // 2C05C writes the inherited start after EB68, even beyond the child
        // clip's end (Genis victory uses112 with the two-frame fallback60).
        // Preserve that stored value through a blend; 8006D2E0 resolves the
        // endpoint on the first advancing visit. Body admission stays bounded.
        let mut clock = Clock::new(
            Playback {
                clip,
                frame: 0.,
                rate: body.rate,
                repeat: body.repeat,
            },
            motion.duration_frames,
            blend,
        )?;
        clock.frame = frame;
        clock.start = frame;
        // EB68 initializes this independently; 2C05C does not copy body loop age.
        animation.clock = clock;
        *current = clip;
        Ok(())
    }

    pub fn step(&mut self, owner: &ModelFrame, advance: bool) -> Result<()> {
        if let Some((clip, animation)) = &mut self.animation {
            if advance {
                animation.advance(&self.definition.skeleton, &self.definition.motions[clip])?;
                self.shown.clip = Some(*clip);
                self.shown.frame = animation.clock.frame;
            }
            self.shown.bones = Arc::new(
                animation
                    .pose(&self.definition.skeleton, [false; 3])?
                    .0
                    .global,
            );
        }
        self.shown.visible = owner.visible && self.visible;
        self.shown.tint = owner.tint;
        self.shown.world = self.detached.unwrap_or_else(|| self.attached_world(owner));
        Ok(())
    }

    fn attached_world(&self, owner: &ModelFrame) -> Matrix {
        multiply(
            owner.world,
            owner.bones[usize::from(self.definition.attachment)],
        )
    }

    pub fn attachment(&self, owner: &ModelFrame) -> [f32; 3] {
        transform_point(self.attached_world(owner), [0.; 3])
    }

    pub fn attached(&self) -> bool {
        self.detached.is_none()
    }

    pub fn detach(&mut self, world: Option<Matrix>) {
        // 155CC composes and draws before the actor callback reaches 21E94.
        // Flight movement and catching change contacts now; the next model
        // visit samples that placement for drawing and ribbon endpoints.
        self.detached = world;
    }

    pub fn anchors(&self, owner: &ModelFrame) -> impl Iterator<Item = [f32; 3]> + '_ {
        let world = self.detached.unwrap_or_else(|| self.attached_world(owner));
        self.definition.anchors.iter().map(move |a| {
            transform_point(
                world,
                transform_point(self.shown.bones[usize::from(a.bone)], a.offset),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::{Bone, Transform, TransformChannels};

    fn owner() -> ModelFrame {
        ModelFrame {
            actor: ActorId(2),
            visible: true,
            tint: [128; 4],
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
            resource: 70,
            attachment: 0,
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
            anchors: vec![Anchor {
                bone: 0,
                offset: [0.; 3],
            }],
            links: vec![],
        }
    }

    #[test]
    fn owner_weapon_keeps_its_entry_phase_duration_and_held_pose() -> Result<()> {
        let mut owner = owner();
        let mut weapon = Weapon::new(Arc::new(definition()), &owner)?;
        assert_eq!(weapon.shown.frame, 0.);
        assert_eq!(weapon.anchors(&owner).next(), Some([111., 222., 333.]));
        weapon.play(
            Playback {
                clip: 30,
                frame: 2.,
                rate: 0.25,
                repeat: false,
            },
            2,
        )?;
        weapon.step(&owner, false)?;
        assert_eq!(weapon.shown.clip, Some(60));
        assert_eq!(weapon.shown.frame, 0.);
        weapon.step(&owner, true)?;
        assert_eq!(weapon.shown.clip, Some(90));
        assert_eq!(weapon.shown.frame, 4.);
        weapon.step(&owner, true)?;
        assert_eq!(weapon.shown.frame, 4.);
        weapon.step(&owner, true)?;
        assert_eq!(weapon.shown.frame, 4.25);
        for _ in 0..100 {
            weapon.step(&owner, true)?;
        }
        assert_eq!(weapon.shown.frame, 20.);
        // Missing clip 61 falls back, with a fresh clock and no body loop offset.
        weapon.play(
            Playback {
                clip: 1,
                frame: 1.,
                rate: 0.5,
                repeat: true,
            },
            0,
        )?;
        for _ in 0..11 {
            weapon.step(&owner, true)?;
        }
        assert_eq!((weapon.shown.clip, weapon.shown.frame), (Some(60), 0.5));
        owner.world[3][0] += 15.;
        weapon.step(&owner, false)?;
        assert_eq!(weapon.shown.frame, 0.5);
        assert_eq!(weapon.anchors(&owner).next(), Some([126., 222., 333.]));
        Ok(())
    }

    #[test]
    fn inherited_start_beyond_child_end_is_stored_until_its_first_advancing_visit() -> Result<()> {
        use resonance_content::animation::{Track, VectorCurve, VectorInterpolation};
        let owner = owner();
        for repeat in [false, true] {
            for blend in [0, 2] {
                let mut definition = definition();
                let motion = definition.motions.get_mut(&60).unwrap();
                motion.duration_frames = 2.;
                motion.tracks = vec![Track {
                    bone: 0,
                    bind_channels: TransformChannels(8),
                    period_frames: 2.,
                    times: vec![0., 2.],
                    translation: Some(VectorCurve {
                        interpolation: VectorInterpolation::Linear,
                        values: vec![[0.; 3], [2., 0., 0.]],
                        incoming: vec![],
                        outgoing: vec![],
                        ease: vec![],
                    }),
                    scale: None,
                    rotation: None,
                    euler_degrees: None,
                    matrices: None,
                }];
                let mut weapon = Weapon::new(Arc::new(definition), &owner)?;
                let held = weapon.shown.clone();
                weapon.play(
                    Playback {
                        clip: 23,
                        frame: 112.,
                        rate: 0.5,
                        repeat,
                    },
                    blend,
                )?;
                let clock = &weapon.animation.as_ref().unwrap().1.clock;
                assert_eq!(
                    (clock.frame, clock.start, clock.end, clock.loop_start),
                    (112., 112., 2., 0.)
                );
                weapon.step(&owner, false)?;
                assert_eq!(weapon.shown, held);
                for _ in 0..blend {
                    weapon.step(&owner, true)?;
                    assert_eq!(weapon.shown.frame, 112.);
                    assert_eq!(weapon.shown.bones[0][3][0], 0.);
                }
                weapon.step(&owner, true)?;
                assert_eq!(weapon.shown.clip, Some(60));
                let end = if repeat { 0. } else { 2. };
                assert_eq!(weapon.shown.frame, end);
                assert_eq!(weapon.shown.bones[0][3][0], end);
                let clock = &weapon.animation.as_ref().unwrap().1.clock;
                assert!(clock.finished);
                assert_eq!(clock.stopped, !repeat);
                weapon.step(&owner, true)?;
                assert_eq!(weapon.shown.frame, if repeat { 0.5 } else { 2. });
            }
        }
        Ok(())
    }

    #[test]
    fn rigid_weapon_uses_live_body_attachment_and_rejects_animated_policy() -> Result<()> {
        let mut definition = definition();
        definition.playback = WeaponPlayback::Rigid;
        assert!(Weapon::new(Arc::new(definition.clone()), &owner()).is_err());
        definition.motions.clear();
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
        assert_eq!(weapon.shown.clip, None);
        assert_eq!(weapon.anchors(&owner).next(), Some([91., 202., 303.]));
        Ok(())
    }

    #[test]
    fn flight_contacts_update_after_draw_and_catching_waits_for_the_next_model_sample() -> Result<()>
    {
        let mut owner = owner();
        let mut weapon = Weapon::new(Arc::new(definition()), &owner)?;
        let carried = weapon.shown.clone();
        let flight = Transform {
            translation: [-40., 5.1, 80.],
            ..Default::default()
        }
        .matrix();
        weapon.detach(Some(flight));
        assert_eq!(weapon.shown, carried);
        assert_eq!(weapon.anchors(&owner).next(), Some([-39., 7.1, 83.]));
        assert!(!weapon.attached());

        // A held animation still draws the preceding flight callback's world.
        weapon.step(&owner, false)?;
        assert_eq!(weapon.shown.world, flight);
        assert_eq!(weapon.shown.frame, carried.frame);
        assert_eq!(weapon.shown.bones, carried.bones);

        owner.world[3][0] += 20.;
        weapon.detach(None);
        assert!(weapon.attached());
        assert_eq!(weapon.anchors(&owner).next(), Some([131., 222., 333.]));
        assert_eq!(weapon.shown.world, flight);
        weapon.step(&owner, false)?;
        assert_eq!(weapon.shown.world, weapon.attached_world(&owner));
        Ok(())
    }
}
