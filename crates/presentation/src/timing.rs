//! Asset preparation does not consume scene or presentation time.
use super::*;
use bevy::{
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d},
    render::{
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, PipelineCache, PollType},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
    },
};

#[cfg(test)]
mod tests;

/// Latched once the startup scene's assets and pipelines have been prepared.
/// The live player and recorder use the same gate before starting either audio
/// or fixed updates. GPU readback preparation is therefore not part of a replay.
#[derive(Resource, Default)]
pub(super) struct Ready(pub bool);

#[allow(clippy::too_many_arguments)] // Startup owners, loaded images, GPU readiness and diagnostics.
pub(super) fn prepare(
    mut ready: ResMut<Ready>,
    field: Res<FieldAssets>,
    renderer: Res<RenderReady>,
    mut art: ResMut<Art>,
    mut images: ResMut<Assets<Image>>,
    mut glows: ResMut<Assets<glow::GlowMaterial>>,
    mut text: ResMut<Assets<TitleText>>,
    server: Res<AssetServer>,
    mut boot: ResMut<boot::Playback>,
    diagnostics: Res<crate::diagnostics::Diagnostics>,
    mut exit: MessageWriter<AppExit>,
) {
    let prepared = (|| -> Result<()> {
        if let Some(error) = &renderer.0.lock().unwrap().error {
            anyhow::bail!("startup draw failed: {error}");
        }
        diagnostics
            .0
            .attempt("startup logos", boot.check_images(&server))?;
        let mut handles: Vec<_> = art
            .images
            .iter()
            .chain(glows.iter().map(|(_, glow)| &glow.texture))
            .cloned()
            .collect();
        let replacements = battle_view::recover_images(
            handles.iter_mut(),
            &server,
            &mut images,
            &diagnostics.0,
            "startup title image",
        )?;
        for image in &mut art.images {
            if let Some(replacement) = replacements.get(&image.id()) {
                *image = replacement.clone();
            }
        }
        let changed: Vec<_> = glows
            .iter()
            .filter_map(|(id, glow)| {
                replacements
                    .get(&glow.texture.id())
                    .map(|image| (id, image.clone()))
            })
            .collect();
        for (id, image) in changed {
            glows.get_mut(id).unwrap().texture = image;
        }
        let changed: Vec<_> = text
            .iter()
            .filter_map(|(id, material)| {
                replacements
                    .get(&material.source.id())
                    .map(|image| (id, image.clone()))
            })
            .collect();
        for (id, image) in changed {
            text.get_mut(id).unwrap().source = image;
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        error!("startup preparation failed: {error:#}");
        exit.write(AppExit::error());
        return;
    }
    ready.0 |= field.ready
        && boot.ready(&server)
        && renderer.0.lock().unwrap().completed.load(Ordering::Acquire)
        && art
            .images
            .iter()
            .chain(glows.iter().map(|(_, glow)| &glow.texture))
            .all(|handle| images.contains(handle));
}

/// Only visible startup draws belong to this fence. Field and battle owners
/// prepare their own draws; their cached pipelines cannot block title return.
#[expect(
    clippy::type_complexity,
    reason = "Bevy queries select the startup roots and visible draws."
)]
pub(super) fn prepare_draws(
    ready: Option<Res<Ready>>,
    renderer: Res<RenderReady>,
    field: Option<Res<FieldAssets>>,
    roots: Query<
        (),
        (
            With<scene::PartRoot>,
            With<WorldAssetRoot>,
            Without<scene::Instantiated>,
        ),
    >,
    draws: Query<
        (Entity, &ViewVisibility, Option<&Mesh2d>, Option<&Mesh3d>),
        Or<(
            With<MeshMaterial2d<TitleOutput>>,
            With<MeshMaterial2d<TitleText>>,
            With<MeshMaterial2d<crate::field_ui::Surface>>,
            With<MeshMaterial3d<materials::TitleSurface>>,
            With<MeshMaterial3d<glow::GlowMaterial>>,
        )>,
    >,
    meshes: Res<Assets<Mesh>>,
) {
    let mut report = renderer.0.lock().unwrap();
    if ready.is_some_and(|ready| ready.0) {
        // A later title activation needs a fresh submission, even if its UI
        // entities are retained from the preceding title visit.
        if report.armed {
            *report = Default::default();
        }
        return;
    }
    if field.is_some_and(|field| !field.ready) || !roots.is_empty() {
        return;
    }
    let expected = draws
        .iter()
        .filter(|(_, visible, mesh2, mesh3)| {
            visible.get()
                && mesh2
                    .map(|mesh| &mesh.0)
                    .or_else(|| mesh3.map(|mesh| &mesh.0))
                    .and_then(|mesh| meshes.get(mesh))
                    .is_none_or(|mesh| mesh.count_vertices() != 0)
        })
        .map(|(entity, ..)| MainEntity::from(entity))
        .collect();
    if !report.armed || report.expected != expected {
        *report = model_preview::gpu::Report {
            armed: true,
            expected,
            ..Default::default()
        };
    }
}

pub(super) fn rendered(
    ready: Res<RenderReady>,
    models: Res<ViewSortedRenderPhases<Transparent3d>>,
    ui: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    diagnostics: Option<Res<crate::diagnostics::Diagnostics>>,
) {
    let mut report = ready.0.lock().unwrap();
    if !report.armed || report.expected.is_empty() || report.error.is_some() {
        return;
    }
    let _ = device.poll(PollType::Poll);
    if report.submitted {
        return;
    }
    let mut prepared = std::collections::HashSet::new();
    for (entity, pipeline) in field_warm::draws(&models, &ui) {
        if !report.expected.contains(&entity) {
            continue;
        }
        match cache.get_render_pipeline_state(pipeline) {
            CachedPipelineState::Ok(_) => {
                prepared.insert(entity);
            }
            CachedPipelineState::Err(error)
                if !crate::model_preview::gpu::shader_pending(error) =>
            {
                if diagnostics.as_ref().is_none_or(|diagnostics| {
                    diagnostics
                        .0
                        .report("draw pipeline", anyhow::anyhow!("{error}"))
                        .is_err()
                }) {
                    report.error = Some(error.to_string());
                    return;
                }
                // Bevy omits the failed draw. Healthy draws still have to be
                // prepared and submitted before startup can advance.
                prepared.insert(entity);
            }
            _ => {}
        }
    }
    if prepared == report.expected {
        report.submitted = true;
        let completed = report.completed.clone();
        queue.on_submitted_work_done(move || completed.store(true, Ordering::Release));
    }
}

#[allow(clippy::too_many_arguments)] // Shared clock with title, movie and field readiness gates.
pub(super) fn advance_clock(
    mut clock: ResMut<Clock>,
    ready: Res<Ready>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    loading: Option<Res<super::loading::Pending>>,
    quickload: Option<Res<super::saves::Quickload>>,
    resident: Option<Res<super::loading::Resident>>,
    mut session: Option<ResMut<super::new_game::Session>>,
    battle: Option<Res<super::battle::Owner>>,
    game_over: Option<Res<super::game_over::Active>>,
) {
    // Presentation age continues across movies and title entries;
    // pure loading waits do not advance it.
    if loading.is_none()
        && quickload.is_none()
        && (battle.as_ref().is_some_and(|battle| battle.presenting())
            || session.is_none()
            || resident
                .as_ref()
                .is_none_or(|r| r.active.load(Ordering::Acquire)))
        && session.as_ref().is_none_or(|session| {
            !session.events().battle_pending()
                || game_over.is_some()
                || battle.as_ref().is_some_and(|battle| battle.presenting())
        })
        && session
            .as_ref()
            .is_none_or(|s| s.events().world.field_transition.is_none())
        && (session.is_some() || ready.0)
        && (boot.active() || !movie.active || movie.is_presenting())
    {
        clock.0.advance();
        if movie.active
            && let Some(session) = &mut session
        {
            session.advance_play_time();
        }
    }
}
