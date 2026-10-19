//! Scene events sample the prepared battle artwork without changing combat playback.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Actor, ActorId, Battle, BattleFrame, ModelDefinition, ModelSample, ModelSampler,
};
use resonance_content::{
    animation::Motion, battle_event::Scene, model_preview::PreviewPart, prepared::Files,
};
use resonance_events::{Face, animation::AnimationSource};
use resonance_game::battle::{
    encounter::{RenderInstances, RenderModel},
    event::Playback,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

type MotionKey = (AnimationSource, u32, u16);
type Selection = (u64, MotionKey, u32, u32);

const BOSS_ALIAS: i32 = 17;
const SOLDIER_ALIASES: [i32; 2] = [100, 101];
const SOLDIER_RESOURCE: u32 = 0x2005d;

struct ActorModel {
    alias: i32,
    id: ActorId,
    resource: u32,
    template: Actor,
    definition: Arc<ModelDefinition>,
    clips: BTreeMap<MotionKey, u16>,
    player: ModelSampler,
    selection: Option<Selection>,
}

pub(super) struct State {
    actors: Vec<ActorModel>,
    combat_count: usize,
    sampled_tick: Option<u32>,
    mouth_cycle: Vec<u8>,
    talking: BTreeMap<i32, u32>,
}

impl State {
    pub fn new(
        files: &Files,
        scenes: &[Arc<Scene>],
        core: &Battle,
        characters: &[u8],
        render: &mut Vec<RenderModel>,
    ) -> Result<Self> {
        let frame = core.snapshot();
        let models = core.models().context("battle event has no actor models")?;
        let boss = frame
            .actors
            .iter()
            .position(|actor| actor.side == resonance_battle::Side::Enemy)
            .context("battle event has no enemy actor")?;
        let mut actors = Vec::new();
        for alias in characters
            .iter()
            .copied()
            .map(i32::from)
            .chain([BOSS_ALIAS].into_iter().chain(SOLDIER_ALIASES))
        {
            let resource = if SOLDIER_ALIASES.contains(&alias) {
                SOLDIER_RESOURCE
            } else {
                alias as u32
            };
            let source = scenes
                .iter()
                .find_map(|scene| scene.actors.get(&resource))
                .with_context(|| format!("missing battle event actor resource {resource:#x}"))?;
            let slot = if let Some(index) = SOLDIER_ALIASES.iter().position(|&id| id == alias) {
                frame.actors.len() + index
            } else if alias == BOSS_ALIAS {
                boss
            } else {
                characters
                    .iter()
                    .position(|&character| i32::from(character) == alias)
                    .context("event character is not in the battle")?
            };
            let id = ActorId::from_index(slot)?;
            let mut template = frame
                .actors
                .get(slot)
                .unwrap_or(&frame.actors[boss])
                .state
                .clone();
            template.availability = resonance_battle::ActorAvailability::Active;
            let mut definition = if slot < frame.actors.len() {
                (**models.definition(id).context("missing battle event body")?).clone()
            } else {
                template.body = Default::default();
                let body = source
                    .parts
                    .first()
                    .context("temporary event actor has no body")?;
                let resource = if let Some(actor) = actors
                    .iter()
                    .find(|actor: &&ActorModel| actor.resource == resource)
                {
                    actor.definition.resource
                } else {
                    let resource = render
                        .iter()
                        .map(|model| model.resource)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .context("battle render resource overflow")?;
                    render.push(RenderModel {
                        resource,
                        skeleton: source.skeleton.clone(),
                        parts: source
                            .parts
                            .iter()
                            .cloned()
                            .map(|scene| PreviewPart {
                                scene,
                                animation: None,
                                attached_to: None,
                                additive: false,
                                uv_offsets: vec![],
                            })
                            .collect(),
                        instances: RenderInstances::Fixed(SOLDIER_ALIASES.len()),
                        lit: true,
                        texture_channels: vec![],
                        weapon_style: None,
                        suppressed_nodes: Default::default(),
                        target_anchor: None,
                    });
                    resource
                };
                ModelDefinition {
                    tint: [
                        resonance_events::effect::NEUTRAL_TINT,
                        resonance_events::effect::NEUTRAL_TINT,
                        resonance_events::effect::NEUTRAL_TINT,
                        u8::MAX,
                    ],
                    fade_on_defeat: false,
                    resource,
                    skeleton: source.skeleton.clone(),
                    body_spheres: vec![],
                    reception_spheres: None,
                    motions: BTreeMap::new(),
                    secondary_motion: body.secondary_motion.chains.clone(),
                    initial: resonance_battle::Playback {
                        clip: 0,
                        frame: 0.,
                        rate: 0.,
                        repeat: false,
                    },
                    reactions: Default::default(),
                    idle_motions: [None; 2],
                    idle_expression: [0; 4],
                    forced_eye_frame: None,
                    blink: None,
                    weapons: vec![],
                    shadow: None,
                    suppress_root_translation: [false; 3],
                }
            };
            let mut clips = BTreeMap::new();
            for external in [false, true] {
                for scene in scenes {
                    let Some(source) = scene.actors.get(&resource) else {
                        continue;
                    };
                    ensure!(
                        source.skeleton.bones.len() == definition.skeleton.bones.len()
                            && source
                                .skeleton
                                .bones
                                .iter()
                                .zip(&definition.skeleton.bones)
                                .all(|(a, b)| a.name == b.name
                                    && a.parent == b.parent
                                    && a.bind == b.bind),
                        "event skeleton differs from combat body"
                    );
                    for clip in source
                        .parts
                        .first()
                        .context("battle event actor has no body")?
                        .clips
                        .iter()
                        .filter(|clip| clip.animation_resource.is_some() == external)
                    {
                        let key = match clip.animation_resource {
                            Some(resource) => {
                                (AnimationSource::Resource, resource, clip.resource_slot)
                            }
                            None => (AnimationSource::Model, resource, clip.resource_slot),
                        };
                        if clips.contains_key(&key) {
                            continue;
                        }
                        let motion = Motion::decode(&files.read(&clip.motion)?)?;
                        motion.validate(&definition.skeleton)?;
                        let slot = if key.0 == AnimationSource::Model {
                            clip.resource_slot
                        } else {
                            definition
                                .motions
                                .keys()
                                .next_back()
                                .copied()
                                .unwrap_or(0)
                                .checked_add(1)
                                .context("event motion slots exhausted")?
                        };
                        definition.motions.insert(slot, motion);
                        clips.insert(key, slot);
                    }
                }
            }
            // Scene actors begin at their own idle pose, independently of combat entry.
            definition.initial = resonance_battle::Playback {
                clip: *clips
                    .get(&(AnimationSource::Model, resource, 0))
                    .context("battle event actor has no initial pose")?,
                frame: 0.,
                rate: 0.,
                repeat: false,
            };
            let definition = Arc::new(definition);
            actors.push(ActorModel {
                alias,
                id,
                resource,
                template: template.clone(),
                clips,
                player: ModelSampler::new(definition.clone(), id, &template)?,
                definition,
                selection: None,
            });
        }
        let mouth_cycle = files
            .json::<resonance_content::effect::FieldEffects>("effects/field.json")?
            .mouth_cycle;
        ensure!(
            !mouth_cycle.is_empty() && mouth_cycle.iter().all(|&frame| frame < 8),
            "invalid event mouth cycle"
        );
        Ok(Self {
            mouth_cycle,
            talking: BTreeMap::new(),
            actors,
            combat_count: frame.actors.len(),
            sampled_tick: None,
        })
    }

    pub fn apply(&mut self, playback: &Playback, frame: &mut BattleFrame) -> Result<()> {
        // Keep the held combat models until the event has its first actor sample.
        if playback.events.tick() == 0 {
            return Ok(());
        }
        let events = &playback.events;
        let tick = events.world.tick;
        let restarted = self.sampled_tick.is_some_and(|previous| tick < previous);
        let advance = self.sampled_tick != Some(tick);
        self.sampled_tick = Some(tick);
        if restarted {
            self.talking.clear();
        }
        let speakers: BTreeSet<_> = playback
            .dialogue
            .iter()
            .filter(|(_, player)| player.is_talking())
            .filter_map(|(slot, player)| {
                events
                    .world
                    .dialogue
                    .get(slot)
                    .filter(|dialogue| dialogue.operation.id() == player.operation.id())
                    .and_then(|dialogue| dialogue.speaker_actor)
            })
            .collect();
        self.talking.retain(|actor, _| speakers.contains(actor));
        for speaker in speakers {
            self.talking.entry(speaker).or_insert(tick);
        }
        frame.models.clear();
        frame.weapons.clear();
        frame.actors.truncate(self.combat_count);
        frame.targets.truncate(self.combat_count);
        for actor in &mut self.actors {
            if actor.id.index() >= self.combat_count {
                frame.actors.push(resonance_battle::ActorFrame {
                    effect_center: actor.template.effect_origin(),
                    state: actor.template.clone(),
                    activity: resonance_battle::Activity::Idle,
                    cast_releasing: false,
                    stun_remaining: 0,
                    stun_struggling: false,
                });
                frame.targets.push(None);
            }
            let Some(source) = events.world.actors.get(&actor.alias) else {
                continue;
            };
            ensure!(
                source.resource == actor.resource,
                "battle event actor changed resource"
            );
            let pose = &mut frame.actors[actor.id.index()];
            let lift = source.visual_position()[2] - source.position[2];
            pose.position = source.position;
            pose.position[1] += lift;
            pose.heading = source.appearance.fixed_heading.unwrap_or(source.heading);
            let scale = source.model_scale();
            ensure!(
                scale[0] == scale[1] && scale[1] == scale[2],
                "nonuniform battle event actor scale"
            );
            pose.body.scale = actor.template.body.scale * scale[0];
            pose.effect_center = pose.effect_origin();
            pose.availability = resonance_battle::ActorAvailability::Active;
            pose.hp = pose.equipment.max_hp;
            pose.overlimit = Default::default();
            if restarted {
                actor.player = ModelSampler::new(actor.definition.clone(), actor.id, pose)?;
                actor.selection = None;
            }
            let (clip, sample, weight, selection) = if let Some(animation) = &source.animation {
                let key = (animation.source, animation.resource, animation.slot);
                let clip = *actor.clips.get(&key).with_context(|| {
                    format!("unprepared event actor {} motion {key:?}", actor.alias)
                })?;
                let duration = actor.definition.motions[&clip].duration_frames;
                let scale =
                    resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ;
                (
                    clip,
                    animation.sample(tick, 0, duration / scale) * scale,
                    animation.blend_weight(tick),
                    Some((
                        source.instance,
                        key,
                        animation.start_tick,
                        animation.phase_tick,
                    )),
                )
            } else {
                (actor.definition.initial.clip, 0., 1., None)
            };
            let (mut model, mut weapons) = actor.player.sample(
                pose,
                ModelSample {
                    clip,
                    frame: sample,
                    blend_weight: weight,
                    restart: actor.selection != selection,
                    rate: source.animation.as_ref().map_or(0., |animation| {
                        animation.rate * resonance_content::animation::FRAME_HZ
                            / resonance_content::ANIMATION_HZ
                    }),
                    repeat: source
                        .animation
                        .as_ref()
                        .is_some_and(|animation| animation.repeat),
                    blend_ticks: source.animation.as_ref().map_or(Ok(0), |animation| {
                        u8::try_from(
                            animation
                                .blend_ticks
                                .saturating_add(u32::from(animation.blend_ticks != 0)),
                        )
                    })?,
                },
                advance,
            )?;
            actor.selection = selection;
            model.visible =
                source.visible && tick >= source.visible_from && !source.appearance.model_hidden;
            model.depth_write = source.depth_write && source.opacity == u8::MAX;
            model.tint = [
                source.tint[0],
                source.tint[1],
                source.tint[2],
                source.opacity,
            ];
            model.texture_layers[0] = events.eye_frame(source);
            model.texture_layers[1] = match source.appearance.mouth {
                Some(Face::Frame(frame)) => frame,
                _ if self.talking.contains_key(&actor.alias) => {
                    self.mouth_cycle[tick.saturating_sub(self.talking[&actor.alias]) as usize
                        % self.mouth_cycle.len()]
                }
                Some(Face::Blink) => self.mouth_cycle[tick as usize % self.mouth_cycle.len()],
                _ => 0,
            };
            for weapon in &mut weapons {
                weapon.visible &= model.visible;
                weapon.tint = model.tint;
            }
            frame.models.push(model);
            frame.weapons.extend(weapons);
        }
        Ok(())
    }
}
