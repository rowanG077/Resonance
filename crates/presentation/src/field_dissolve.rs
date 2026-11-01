//! Blend a captured scene into the live image for dissolves and motion trails.
use bevy::{
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
        view::ViewTarget,
    },
};

#[derive(Component, Clone, ExtractComponent, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
struct Settings {
    opacity: Vec4,
}

#[derive(Component, Clone, Copy, ExtractComponent)]
#[extract_app(bevy::render::RenderApp)]
struct FadingOut(bool);

#[derive(Component, Clone, Copy, ExtractComponent)]
#[extract_app(bevy::render::RenderApp)]
struct BattleCapture {
    update: u64,
    generation: Option<Generation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Generation {
    Enemy(u64),
    Unison(u64),
    Activation(u64),
}

/// Over Limit and rescue bursts share a short, expanding scene history.
#[derive(Default)]
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
pub(super) fn battle(
    commands: &mut Commands,
    camera: Entity,
    frame: &resonance_battle::BattleFrame,
    activation: &ActivationFeedback,
) {
    let dissolve = frame.enemy_presentation.as_ref().map_or_else(
        || frame.summon.as_ref().map_or(0, |summon| summon.dissolve),
        |enemy| enemy.dissolve,
    );
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
    let expansion = feedback.map_or(0., |(feedback, _, _)| f32::from(feedback.expansion));
    commands.entity(camera).insert((
        Settings {
            opacity: Vec4::new(
                feedback.map_or(f32::from(dissolve), |(feedback, _, _)| {
                    f32::from(feedback.alpha)
                }) / 255.,
                0.,
                640. / (640. + expansion * 2.),
                480. / (480. + expansion * 2.),
            ),
        },
        BattleCapture {
            update: feedback.map_or(frame.update, |(_, _, update)| update),
            generation: feedback.map(|(_, generation, _)| generation),
        },
    ));
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Pass;

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "field_dissolve.wesl");
    app.add_plugins((
        ExtractComponentPlugin::<Settings>::default(),
        ExtractComponentPlugin::<FadingOut>::default(),
        ExtractComponentPlugin::<BattleCapture>::default(),
        UniformComponentPlugin::<Settings>::default(),
    ))
    .add_systems(PostUpdate, sync);
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Core3d,
            render
                .in_set(Core3dSystems::PostProcess)
                .in_set(Pass)
                .after(super::field_refraction::Pass),
        );
}

fn sync(
    mut commands: Commands,
    art: Option<Res<super::field_view::Art>>,
    state: super::field_view::State,
    views: Query<Entity, With<super::FieldCamera>>,
) {
    if art.is_none() {
        for view in &views {
            commands.entity(view).remove::<(Settings, FadingOut)>();
        }
        return;
    }
    let world = &state.get().events.world;
    let alpha = world
        .scene_dissolve
        .as_ref()
        .map_or(0., |f| f.alpha(world.tick));
    let feedback = alpha == 0. && world.frame_feedback != 0;
    let fading_out = world
        .fade
        .as_ref()
        .is_some_and(|fade| fade.to > fade.from && fade.alpha(world.tick) > 0.);
    for view in &views {
        commands.entity(view).insert(FadingOut(fading_out));
        commands.entity(view).insert(Settings {
            opacity: Vec4::new(
                if feedback { 128. / 255. } else { alpha / 255. },
                f32::from(feedback),
                1.,
                1.,
            ),
        });
    }
}

#[derive(Resource)]
pub(super) struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
impl Pipeline {
    pub(super) fn ready(&self, cache: &PipelineCache) -> bool {
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
                format: TextureFormat::Bgra8Unorm,
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
    /// Capture once per battle update; drawing the same update must not feed back again.
    fn advance(&mut self, frame: BattleCapture) -> (bool, bool) {
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
    next: Texture,
    binding: Option<(BufferId, BindGroup)>,
    cursor: FeedbackCursor,
}

struct History {
    camera: Entity,
    texture: Texture,
    binding: Option<(BufferId, BindGroup)>,
    feedback: Option<FeedbackHistory>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn render(
    view: ViewQuery<(
        Entity,
        &ViewTarget,
        &Settings,
        &DynamicUniformIndex<Settings>,
        Option<&BattleCapture>,
        Option<&FadingOut>,
        Option<&super::menu_backdrop::SceneCapture>,
    )>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Settings>>,
    images: Res<RenderAssets<GpuImage>>,
    mut history: Local<Option<History>>,
    mut context: RenderContext,
) {
    let (camera, target, settings, index, battle, fade, capture) = view.into_inner();
    let fading_out = fade.is_some_and(|fade| fade.0);
    let Some(gpu_pipeline) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    let Some(buffer) = uniforms.uniforms().buffer() else {
        return;
    };
    let source = target.main_texture();
    let replace = history
        .as_ref()
        .is_none_or(|h| h.camera != camera || h.texture.size() != source.size());
    if replace {
        let texture = context.render_device().create_texture(&TextureDescriptor {
            label: Some("resonance/dissolve-history"),
            size: source.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: source.format(),
            usage: TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST
                | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        *history = Some(History {
            camera,
            texture,
            binding: None,
            feedback: None,
        });
    }
    let history = history.as_mut().unwrap();
    // Scripts may retire actors under a fade before requesting battle. Keep
    // the last visible scene for the shatter instead of exposing that cleanup.
    if fading_out
        && !replace
        && let Some(capture) = capture
    {
        capture.copy(&history.texture, &images, &mut context);
    }
    let feedback = battle.filter(|frame| frame.generation.is_some());
    let mut capture_feedback = false;
    if let Some(frame) = feedback {
        let next = history.feedback.get_or_insert_with(|| FeedbackHistory {
            next: context.render_device().create_texture(&TextureDescriptor {
                label: Some("resonance/battle-feedback-next"),
                size: source.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: source.format(),
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
            binding: None,
            cursor: Default::default(),
        });
        let (changed, fresh) = next.cursor.advance(*frame);
        capture_feedback = changed;
        if changed && !fresh {
            std::mem::swap(&mut history.texture, &mut next.next);
            std::mem::swap(&mut history.binding, &mut next.binding);
        }
        if next.cursor.capture_only {
            if changed {
                context.command_encoder().copy_texture_to_texture(
                    source.as_image_copy(),
                    next.next.as_image_copy(),
                    source.size(),
                );
            }
            return;
        }
    } else if let Some(feedback) = &mut history.feedback {
        feedback.cursor.generation = None;
    }
    if replace || settings.opacity.x == 0. {
        if replace || !fading_out {
            context.command_encoder().copy_texture_to_texture(
                source.as_image_copy(),
                history.texture.as_image_copy(),
                source.size(),
            );
        }
        return;
    }
    if history
        .binding
        .as_ref()
        .is_none_or(|(id, _)| *id != buffer.id())
    {
        let texture_view = history
            .texture
            .create_view(&TextureViewDescriptor::default());
        history.binding = Some((
            buffer.id(),
            context.render_device().create_bind_group(
                "resonance/dissolve",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    &texture_view,
                    &pipeline.sampler,
                    uniforms.uniforms().binding().unwrap(),
                )),
            ),
        ));
    }
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/dissolve"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target.main_texture_view(),
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
    pass.set_bind_group(0, &history.binding.as_ref().unwrap().1, &[index.index()]);
    pass.draw(0..3, 0..1);
    drop(pass);
    if capture_feedback {
        context.command_encoder().copy_texture_to_texture(
            source.as_image_copy(),
            history.feedback.as_ref().unwrap().next.as_image_copy(),
            source.size(),
        );
    } else if settings.opacity.y != 0. && !fading_out {
        context.command_encoder().copy_texture_to_texture(
            source.as_image_copy(),
            history.texture.as_image_copy(),
            source.size(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_feedback_captures_once_per_update_and_refreshes_each_generation() {
        use resonance_battle::{ActorId, BattleClock, BattleFrame, Cue, RescueKind};
        let mut cursor = FeedbackCursor::default();
        let mut frame = BattleCapture {
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
    }
}
