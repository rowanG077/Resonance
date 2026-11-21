//! Blend a captured scene into the live image for dissolves and motion trails.
use bevy::{
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::RenderDevice,
    },
};

#[derive(Component, Clone, ShaderType)]
pub(crate) struct Settings {
    opacity: Vec4,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct FadingOut(bool);

/// Whether the scene image before UI must be kept this frame: a dissolve,
/// fade or feedback runs now or starts at the predicted next update.
///
/// Keeping it takes a separate scene pass, so the native frame draws the scene
/// and its overlay in one pass when nothing will read the kept image. Cameras
/// without this component keep it every frame.
#[derive(Component, Clone, Copy)]
pub(crate) struct KeepHistory(bool);

#[derive(Component, Clone, Copy)]
pub(crate) struct FeedbackFrame {
    update: u64,
    generation: Option<Generation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generation {
    Field(u32),
    Enemy(u64),
    Unison(u64),
    Activation(u64),
}

/// Over Limit and rescue bursts share a short, expanding scene history.
#[derive(Default, Clone)]
pub(super) struct ActivationFeedback {
    update: Option<u64>,
    frame: Option<resonance_battle::UnisonFeedback>,
}

impl ActivationFeedback {
    pub(super) fn advance(&mut self, frame: &resonance_battle::BattleFrame) {
        use resonance_battle::{BattleClock, Cue, UnisonFeedback};
        if self.update == Some(frame.update) {
            return;
        }
        if !matches!(
            frame.clock,
            BattleClock::Running | BattleClock::TimedHold | BattleClock::Rescue(_)
        ) {
            return;
        }
        self.update = Some(frame.update);
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::OverLimitEntered { .. } | Cue::Rescued { .. }))
        {
            self.frame = Some(UnisonFeedback {
                generation: frame.update,
                expansion: self.frame.map_or(0, |feedback| feedback.expansion),
                alpha: 176,
                limit: 2,
                remaining: 45,
            });
        }
        if let Some(feedback) = &mut self.frame {
            if feedback.remaining > 0 {
                feedback.remaining -= 1;
                feedback.expansion = (feedback.expansion + 1).min(feedback.limit);
            } else {
                feedback.expansion = feedback.expansion.saturating_sub(1);
                if feedback.expansion == 0 {
                    self.frame = None;
                }
            }
        }
    }
}

/// Share the scene capture pass with battle dissolves and fixed-update feedback.
///
/// `next` is the predicted next update. The kept scene image must already hold
/// the previous frame when a dissolve or feedback starts there, so without a
/// prediction it is kept. A deferred prediction may not run before another
/// committed tick, especially near 60 FPS or after a slow frame.
pub(super) fn battle(
    commands: &mut Commands,
    camera: Entity,
    frame: &resonance_battle::BattleFrame,
    next: Option<&resonance_battle::BattleFrame>,
    alpha: f32,
    activation: &ActivationFeedback,
) {
    let (dissolve, feedback) = battle_effects(frame, activation);
    let future = next.map(|next| {
        let mut predicted = activation.clone();
        predicted.advance(next);
        battle_effects(next, &predicted)
    });
    let keep =
        dissolve != 0 || feedback.is_some() || future.is_none_or(|(d, f)| d != 0 || f.is_some());
    let mut opacity = feedback.map_or(f32::from(dissolve), |(f, _, _)| f32::from(f.alpha));
    let mut expansion = feedback.map_or(0., |(f, _, _)| f32::from(f.expansion));
    if let Some((next_dissolve, next_feedback)) = future {
        match (feedback, next_feedback) {
            (Some((_, id, _)), Some((f, next_id, _))) if id == next_id => {
                opacity += (f32::from(f.alpha) - opacity) * alpha;
                expansion += (f32::from(f.expansion) - expansion) * alpha;
            }
            (Some((f, _, _)), None)
                if next_dissolve == 0 && f.remaining == 0 && f.expansion == 1 =>
            {
                opacity *= 1. - alpha;
                expansion *= 1. - alpha;
            }
            (None, None)
                if dissolve > 0
                    && next.is_some_and(|next| {
                        frame.summon.map(|s| s.generation) == next.summon.map(|s| s.generation)
                            && frame.enemy_presentation.is_some()
                                == next.enemy_presentation.is_some()
                            && next_dissolve <= dissolve
                    }) =>
            {
                opacity += (f32::from(next_dissolve) - opacity) * alpha;
            }
            _ => {}
        }
    }
    commands.entity(camera).insert((
        Settings {
            opacity: Vec4::new(
                opacity / 255.,
                0.,
                640. / (640. + expansion * 2.),
                480. / (480. + expansion * 2.),
            ),
        },
        FeedbackFrame {
            update: feedback.map_or(frame.update, |(_, _, update)| update),
            generation: feedback.map(|(_, generation, _)| generation),
        },
        KeepHistory(keep),
    ));
}

/// The dissolve opacity and the feedback, with its generation and update, that
/// `frame` shows.
fn battle_effects(
    frame: &resonance_battle::BattleFrame,
    activation: &ActivationFeedback,
) -> (
    u8,
    Option<(resonance_battle::UnisonFeedback, Generation, u64)>,
) {
    let dissolve = frame
        .enemy_presentation
        .as_ref()
        .map_or(0, |enemy| enemy.dissolve)
        .max(frame.summon.as_ref().map_or(0, |summon| summon.dissolve))
        .max(frame.event_dissolve);
    let feedback = frame
        .enemy_presentation
        .as_ref()
        .and_then(|enemy| enemy.feedback)
        .map(|feedback| {
            (
                feedback,
                Generation::Enemy(feedback.generation),
                frame.update,
            )
        })
        .or_else(|| {
            frame
                .unison
                .as_ref()
                .and_then(|unison| unison.feedback)
                .map(|feedback| {
                    (
                        feedback,
                        Generation::Unison(feedback.generation),
                        frame.update,
                    )
                })
        })
        .or_else(|| {
            activation
                .frame
                .zip(activation.update)
                .map(|(feedback, update)| {
                    (
                        feedback,
                        Generation::Activation(feedback.generation),
                        update,
                    )
                })
        })
        .filter(|(feedback, _, _)| dissolve == 0 && feedback.expansion != 0);
    (dissolve, feedback)
}

/// Whether a field world reads or freezes the kept scene image. Fade-outs
/// freeze it; a finished fade-in stays in the world but reads nothing.
fn field_keeps_history(world: &resonance_events::GameWorld) -> bool {
    world
        .scene_dissolve
        .as_ref()
        .is_some_and(|d| d.alpha(world.tick) > 0.)
        || world.frame_feedback != 0
        || world.fade.as_ref().is_some_and(|fade| fade.to > fade.from)
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "field_dissolve.wesl");
    app.add_systems(PostUpdate, sync);
    app.sub_app_mut(RenderApp)
        .init_resource::<SceneHistory>()
        .add_systems(RenderStartup, prepare);
}

fn sync(
    mut commands: Commands,
    art: Option<Res<super::field_view::Art>>,
    state: super::field_view::State,
    prediction: Option<Res<crate::field_view::Prediction>>,
    views: Query<Entity, With<super::FieldCamera>>,
) {
    // Unpublished warm-up frames must not enter the next visible dissolve or trail.
    if art.is_none_or(|art| art.retaining_output) {
        for view in &views {
            commands
                .entity(view)
                .remove::<(Settings, FadingOut, KeepHistory, FeedbackFrame)>();
        }
        return;
    }
    let field = state.get();
    let next = prediction
        .as_ref()
        .and_then(|p| p.field().map(|f| (&f.events.world, p.alpha)));
    for view in &views {
        commands
            .entity(view)
            .insert(field_settings(field.map_id, &field.events.world, next));
    }
}

pub(super) fn field_settings(
    map: u32,
    world: &resonance_events::GameWorld,
    next: Option<(&resonance_events::GameWorld, f32)>,
) -> (Settings, FadingOut, KeepHistory, FeedbackFrame) {
    // Keep the preceding scene when the next update's effects are unknown.
    let keep =
        field_keeps_history(world) || next.is_none_or(|(world, _)| field_keeps_history(world));
    let mut alpha = world
        .scene_dissolve
        .as_ref()
        .map_or(0., |f| f.alpha(world.tick));
    if let Some((next, a)) = next
        && let Some(dissolve) = &world.scene_dissolve
        && next.scene_dissolve.as_ref() == Some(dissolve)
    {
        alpha += (dissolve.alpha(next.tick) - alpha) * a;
    }
    let feedback = alpha == 0. && world.frame_feedback != 0;
    let fading_out = world
        .fade
        .as_ref()
        .is_some_and(|fade| fade.to > fade.from && fade.alpha(world.tick) > 0.);
    (
        Settings {
            opacity: Vec4::new(
                if feedback { 128. / 255. } else { alpha / 255. },
                0.,
                1.,
                1.,
            ),
        },
        FadingOut(fading_out),
        KeepHistory(keep),
        FeedbackFrame {
            update: u64::from(world.tick),
            generation: (feedback && !fading_out).then_some(Generation::Field(map)),
        },
    )
}

#[derive(Resource)]
pub(crate) struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
impl Pipeline {
    pub(crate) fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.id).is_some()
    }
}
fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/dissolve",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<Settings>(true),
            ),
        ),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resonance/dissolve".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: server.load("embedded://resonance_presentation/field_dissolve.wesl"),
            targets: vec![Some(ColorTargetState {
                format: crate::renderer::SCENE_COLOR_FORMAT,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(Pipeline {
        layout,
        id,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
    });
}

#[derive(Default)]
struct FeedbackCursor {
    generation: Option<Generation>,
    update: u64,
    capture_only: bool,
}

impl FeedbackCursor {
    /// Advance the feedback source once per committed update.
    fn advance(&mut self, frame: FeedbackFrame) -> (bool, bool) {
        let fresh = self.generation != frame.generation || frame.update < self.update;
        let changed = fresh || self.update != frame.update;
        if changed {
            self.generation = frame.generation;
            self.update = frame.update;
            self.capture_only = fresh;
        }
        (changed, fresh)
    }
}

struct FeedbackHistory {
    texture: Texture,
    binding: Option<(BufferId, BindGroup)>,
    cursor: FeedbackCursor,
}

struct History {
    /// Main-world camera entity whose scene this keeps.
    camera: Entity,
    texture: Texture,
    /// The native frame's view of `texture`, made when it first needs one.
    view: Option<TextureView>,
    binding: Option<(BufferId, BindGroup)>,
    feedback: Option<FeedbackHistory>,
    /// Whether `texture` holds the previous frame's scene, or the image a
    /// fade or dissolve has held since. A frame drawn without keeping its
    /// scene clears it, and the next use starts over from the current scene.
    valid: bool,
}

/// The last scene image before UI, kept for dissolves and feedback.
#[derive(Resource, Default)]
pub(crate) struct SceneHistory(Option<History>);

const HISTORY_USAGES: TextureUsages = TextureUsages::COPY_SRC
    .union(TextureUsages::COPY_DST)
    .union(TextureUsages::TEXTURE_BINDING)
    .union(TextureUsages::RENDER_ATTACHMENT);

/// The sRGB view the native frame's SMAA blends through.
fn srgb_view(format: &TextureFormat) -> &[TextureFormat] {
    match format {
        TextureFormat::Bgra8Unorm => &[TextureFormat::Bgra8UnormSrgb],
        TextureFormat::Rgba8Unorm => &[TextureFormat::Rgba8UnormSrgb],
        _ => &[],
    }
}

fn history_texture(device: &RenderDevice, label: &'static str, like: &Texture) -> Texture {
    let format = like.format();
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: like.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: HISTORY_USAGES,
        view_formats: srgb_view(&format),
    })
}

/// One scene view's dissolve inputs for the native frame.
#[derive(Clone, Copy)]
pub(crate) struct NativeDissolve {
    pub camera: Entity,
    pub opacity: Vec4,
    /// See [`KeepHistory`].
    pub keep: bool,
    fading_out: bool,
    feedback: Option<FeedbackFrame>,
}
pub(crate) type NativeQuery = (
    &'static Settings,
    Option<&'static FadingOut>,
    Option<&'static FeedbackFrame>,
    Option<&'static KeepHistory>,
);
impl NativeDissolve {
    pub(crate) fn fading_out(&self) -> bool {
        self.fading_out
    }

    pub(crate) fn new(
        camera: Entity,
        (settings, fade, feedback, keep): (
            &Settings,
            Option<&FadingOut>,
            Option<&FeedbackFrame>,
            Option<&KeepHistory>,
        ),
    ) -> Self {
        Self {
            camera,
            opacity: settings.opacity,
            keep: keep.is_none_or(|keep| keep.0),
            fading_out: fade.is_some_and(|fade| fade.0),
            feedback: feedback.copied(),
        }
    }
}

/// A single-sample scene image the native frame resolved into.
pub(crate) struct NativeScene {
    pub texture: Texture,
    pub view: TextureView,
}
impl NativeScene {
    pub(crate) fn new(device: &RenderDevice, size: Extent3d, format: TextureFormat) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("resonance/native-scene"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: HISTORY_USAGES,
            view_formats: srgb_view(&format),
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        Self { texture, view }
    }
}

impl SceneHistory {
    /// The frame did not keep its scene: the kept image is no longer the
    /// previous frame's, so its next use starts over.
    pub(crate) fn invalidate(&mut self) {
        if let Some(history) = &mut self.0 {
            history.valid = false;
        }
    }

    /// The kept image of the previous frame, while it is valid.
    pub(crate) fn valid_texture(&self) -> Option<&Texture> {
        self.0
            .as_ref()
            .filter(|history| history.valid)
            .map(|history| &history.texture)
    }

    /// The texture behind a view [`Self::native`] returned other than the
    /// scene's own: the image it just kept.
    pub(crate) fn kept_texture(&self) -> Option<&Texture> {
        self.0.as_ref().map(|history| &history.texture)
    }

    /// Blend the kept scene over the one just resolved and keep this one for
    /// later: this exchanges the two textures, leaving `scene` as the next
    /// frame's target. Returns the view of the image to composite UI over.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn native(
        &mut self,
        dissolve: &NativeDissolve,
        scene: &mut NativeScene,
        device: &RenderDevice,
        cache: &PipelineCache,
        pipeline: &Pipeline,
        encoder: &mut CommandEncoder,
        uniforms: &Buffer,
        offset: u32,
    ) -> TextureView {
        let replace = self.0.as_ref().is_none_or(|h| {
            h.camera != dissolve.camera
                || h.texture.size() != scene.texture.size()
                || h.texture.format() != scene.texture.format()
                || !h.texture.usage().contains(HISTORY_USAGES)
        });
        if replace {
            self.0 = Some(History {
                camera: dissolve.camera,
                texture: history_texture(device, "resonance/dissolve-history", &scene.texture),
                view: None,
                binding: None,
                feedback: None,
                valid: false,
            });
        }
        let history = self.0.as_mut().unwrap();
        let reset = !history.valid;
        if reset && !replace && (dissolve.opacity.x != 0. || dissolve.fading_out) {
            debug!("dissolve history restarts from the current scene: its start was not predicted");
        }
        // Exchange instead of copying: the kept image becomes the next render
        // target and the scene just drawn becomes the history.
        fn keep(history: &mut History, scene: &mut NativeScene) -> TextureView {
            let target = history.view.take().unwrap_or_else(|| {
                history
                    .texture
                    .create_view(&TextureViewDescriptor::default())
            });
            std::mem::swap(&mut history.texture, &mut scene.texture);
            let kept = std::mem::replace(&mut scene.view, target);
            history.view = Some(kept.clone());
            history.binding = None;
            history.valid = true;
            kept
        }
        let feedback = dissolve.feedback.filter(|frame| frame.generation.is_some());
        let field_feedback =
            feedback.is_some_and(|frame| matches!(frame.generation, Some(Generation::Field(_))));
        let mut capture_feedback = false;
        if let Some(frame) = feedback {
            let buffer = history.feedback.get_or_insert_with(|| FeedbackHistory {
                texture: history_texture(device, "resonance/feedback-history", &scene.texture),
                binding: None,
                cursor: Default::default(),
            });
            if field_feedback && reset {
                buffer.cursor.generation = None;
                return keep(history, scene);
            }
            let (changed, fresh) = buffer.cursor.advance(frame);
            if changed && (field_feedback || !fresh) {
                // Battle promotes its staged capture. Field puts the last
                // displayed scene in the buffer as this tick's fixed source.
                std::mem::swap(&mut history.texture, &mut buffer.texture);
                std::mem::swap(&mut history.binding, &mut buffer.binding);
                history.view = None;
            }
            if !field_feedback {
                capture_feedback = changed;
                if buffer.cursor.capture_only {
                    if changed {
                        encoder.copy_texture_to_texture(
                            scene.texture.as_image_copy(),
                            buffer.texture.as_image_copy(),
                            scene.texture.size(),
                        );
                    }
                    return scene.view.clone();
                }
            }
        } else if let Some(feedback) = &mut history.feedback {
            feedback.cursor.generation = None;
        }
        if reset || dissolve.opacity.x == 0. {
            if reset || !dissolve.fading_out {
                return keep(history, scene);
            }
            return scene.view.clone();
        }
        let Some(gpu_pipeline) = cache.get_render_pipeline(pipeline.id) else {
            return if field_feedback {
                keep(history, scene)
            } else {
                scene.view.clone()
            };
        };
        let (source, binding) = if field_feedback {
            let feedback = history.feedback.as_mut().unwrap();
            (&feedback.texture, &mut feedback.binding)
        } else {
            (&history.texture, &mut history.binding)
        };
        if binding.as_ref().is_none_or(|(id, _)| *id != uniforms.id()) {
            let texture_view = source.create_view(&TextureViewDescriptor::default());
            *binding = Some((
                uniforms.id(),
                device.create_bind_group(
                    "resonance/dissolve",
                    &cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((
                        &texture_view,
                        &pipeline.sampler,
                        BufferBinding {
                            buffer: uniforms,
                            offset: 0,
                            size: std::num::NonZeroU64::new(16),
                        },
                    )),
                ),
            ));
        }
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("resonance/dissolve"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &scene.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
            pass.set_pipeline(gpu_pipeline);
            pass.set_bind_group(0, &binding.as_ref().unwrap().1, &[offset]);
            pass.draw(0..3, 0..1);
        }
        if capture_feedback {
            encoder.copy_texture_to_texture(
                scene.texture.as_image_copy(),
                history.feedback.as_ref().unwrap().texture.as_image_copy(),
                scene.texture.size(),
            );
        } else if field_feedback {
            return keep(history, scene);
        }
        scene.view.clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_feedback_captures_once_per_update_and_refreshes_each_generation() {
        use resonance_battle::{ActorId, BattleClock, BattleFrame, Cue, RescueKind};
        let mut cursor = FeedbackCursor::default();
        let mut frame = FeedbackFrame {
            update: 10,
            generation: Some(Generation::Enemy(1)),
        };
        assert_eq!(cursor.advance(frame), (true, true));
        assert!(cursor.capture_only);
        assert_eq!(cursor.advance(frame), (false, false));
        assert!(cursor.capture_only);
        frame.update += 1;
        assert_eq!(cursor.advance(frame), (true, false));
        assert!(!cursor.capture_only);
        assert_eq!(cursor.advance(frame), (false, false));
        frame.generation = Some(Generation::Activation(1));
        assert_eq!(cursor.advance(frame), (true, true));
        assert!(cursor.capture_only);
        cursor.generation = None;
        assert_eq!(cursor.advance(frame), (true, true));

        let actor = ActorId::from_index(0).unwrap();
        let mut activation = ActivationFeedback::default();
        let mut frame = BattleFrame {
            update: 20,
            cues: vec![Cue::OverLimitEntered {
                actor,
                position: [0.; 3],
            }],
            ..Default::default()
        };
        activation.advance(&frame);
        let first = activation.frame.unwrap();
        assert_eq!(
            (first.expansion, first.remaining, first.alpha),
            (1, 44, 176)
        );
        activation.advance(&frame);
        assert_eq!(activation.frame, Some(first));

        frame.cues.clear();
        frame.update += 1;
        frame.clock = BattleClock::Held;
        activation.advance(&frame);
        assert_eq!(activation.frame, Some(first));
        assert_eq!(activation.update, Some(20));
        frame.update += 1;
        frame.clock = BattleClock::TimedHold;
        activation.advance(&frame);
        let second = activation.frame.unwrap();
        assert_eq!((second.expansion, second.remaining), (2, 43));

        frame.update += 1;
        frame.clock = BattleClock::Rescue(actor);
        frame.cues.push(Cue::Rescued {
            actor,
            kind: RescueKind::Revive,
        });
        activation.advance(&frame);
        let restarted = activation.frame.unwrap();
        assert_eq!((restarted.expansion, restarted.remaining), (2, 44));
        assert_ne!(restarted.generation, first.generation);
        frame.cues.clear();
        for _ in 0..44 {
            frame.update += 1;
            activation.advance(&frame);
        }
        assert_eq!(activation.frame.unwrap().remaining, 0);
        frame.update += 1;
        activation.advance(&frame);
        assert_eq!(activation.frame.unwrap().expansion, 1);
        frame.update += 1;
        activation.advance(&frame);
        assert!(activation.frame.is_none());

        // A retained boss presentation with no active dissolve must not hide
        // the summon or event transition that currently owns the camera.
        let mut world = World::new();
        let camera = world.spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        frame.enemy_presentation = Some(resonance_battle::EnemyPresentationFrame {
            hide_party: false,
            hide_hud: false,
            dissolve: 0,
            flash: None,
            feedback: None,
            letterbox: 0,
        });
        frame.summon = Some(resonance_battle::SummonFrame {
            owner: Some(actor),
            age: Some(0),
            hide_hud: true,
            generation: 1,
            dissolve: 200,
            flash: 0,
            flash_active: false,
        });
        for (event_dissolve, next_event, expected) in
            [(0, 0, 195.), (255, 247, 251.), (230, 255, 230.)]
        {
            frame.event_dissolve = event_dissolve;
            let mut next = frame.clone();
            next.event_dissolve = next_event;
            next.summon.as_mut().unwrap().dissolve -= 10;
            battle(
                &mut Commands::new(&mut queue, &world),
                camera,
                &frame,
                Some(&next),
                0.5,
                &activation,
            );
            queue.apply(&mut world);
            assert_eq!(
                world.get::<Settings>(camera).unwrap().opacity.x,
                expected / 255.
            );
        }
    }

    #[test]
    fn scene_history_is_kept_when_a_dissolve_runs_or_may_start_next_update() {
        use resonance_battle::{ActorId, BattleClock, BattleFrame, Cue};
        let keep = |frame: &BattleFrame, next: Option<&BattleFrame>| {
            let mut world = World::new();
            let camera = world.spawn_empty().id();
            let mut queue = bevy::ecs::world::CommandQueue::default();
            battle(
                &mut Commands::new(&mut queue, &world),
                camera,
                frame,
                next,
                0.5,
                &ActivationFeedback::default(),
            );
            queue.apply(&mut world);
            world.get::<KeepHistory>(camera).unwrap().0
        };
        let quiet = BattleFrame {
            clock: BattleClock::Running,
            ..Default::default()
        };
        let mut dissolving = quiet.clone();
        dissolving.event_dissolve = 64;
        let mut bursting = quiet.clone();
        bursting.update = 1;
        bursting.cues.push(Cue::OverLimitEntered {
            actor: ActorId::from_index(0).unwrap(),
            position: [0.; 3],
        });

        assert!(!keep(&quiet, Some(&quiet)));
        assert!(keep(&dissolving, Some(&quiet)));
        // The kept image must hold the frame before the update that starts it.
        assert!(keep(&quiet, Some(&dissolving)));
        assert!(keep(&quiet, Some(&bursting)));
        // Includes deferred or disabled prediction: the next frame may tick again.
        assert!(keep(&quiet, None));
    }

    #[test]
    fn field_history_follows_feedback_fades_and_dissolve_replacements() {
        let mut world = resonance_events::GameWorld::default();
        assert!(!field_keeps_history(&world));
        let fade = |from, to| resonance_events::Fade {
            start_tick: 0,
            duration: 30,
            from,
            to,
            white: false,
        };
        // A finished fade-in stays in the world and reads no history.
        world.fade = Some(fade(255., 0.));
        assert!(!field_keeps_history(&world));
        world.fade = Some(fade(0., 255.));
        assert!(field_keeps_history(&world));
        world.fade = None;
        world.frame_feedback = 1;
        assert!(field_keeps_history(&world));

        world.frame_feedback = 0;
        let dissolve = |start_tick| resonance_events::SceneDissolve {
            start_tick,
            duration: 4,
        };
        world.scene_dissolve = Some(dissolve(0));
        let mut next = resonance_events::GameWorld::default();
        for (tick, next_start, expected_alpha, keep) in [
            (1, 0, 159., true), // Midway between 191 and 127 on the same dissolve.
            (1, 2, 191., true), // Hold the old image until the replacement commits.
            (4, 0, 0., false),  // A completed dissolve remains in the world.
            (4, 5, 0., true),   // Capture the scene before the next dissolve starts.
        ] {
            world.tick = tick;
            next.tick = tick + 1;
            next.scene_dissolve = Some(dissolve(next_start));
            let (settings, _, history, _) = field_settings(0, &world, Some((&next, 0.5)));
            assert_eq!(
                (settings.opacity.x, history.0),
                (expected_alpha / 255., keep)
            );
        }
        assert!(field_settings(0, &world, None).2.0);
    }
}
