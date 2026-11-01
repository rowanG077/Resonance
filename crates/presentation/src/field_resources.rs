//! A script-selected resource is prepared while its requesting VM is suspended.
use super::{
    field_view::{Art, Failures, Part},
    loading::{Resident, Task},
    new_game::Session,
};
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::prelude::*;
use resonance_content::{
    field::{FieldAssets, document::Actors},
    prepared::Files,
};
use resonance_events::{ResourceLibrary, ResourceRequest};
use std::sync::{Arc, atomic::Ordering};

pub(super) struct Prepared {
    assets: FieldAssets,
    files: Arc<Files>,
    resources: ResourceLibrary,
    changed: Vec<u32>,
}
impl Prepared {
    fn preserve_resident_files(&mut self, resident: &Resident) {
        // Equipment and script resources can finish in either order.
        if let Some(current) = resident.files.read().unwrap().as_ref() {
            let files = Arc::make_mut(&mut self.files);
            for (path, bytes) in current.iter() {
                if !files.contains_key(path) {
                    files.insert(path.clone(), bytes.clone());
                }
            }
        }
    }
}
pub(super) enum Pending {
    Capturing(ResourceRequest, Task<Prepared>),
    Loading(ResourceRequest, Prepared),
}
impl Pending {
    fn request(&self) -> &ResourceRequest {
        match self {
            Self::Capturing(request, _) | Self::Loading(request, _) => request,
        }
    }
}

pub(super) fn update(
    session: Option<ResMut<Session>>,
    art: Option<ResMut<Art>>,
    resident: Res<Resident>,
    server: Res<AssetServer>,
    mut pending: Local<Option<Pending>>,
    mut failures: Failures,
) {
    let Some(mut session) = session.filter(|session| session.is_field()) else {
        *pending = None;
        return;
    };
    let Some(mut art) = art else { return };
    if art.map != session.map_id() {
        return;
    }
    if pending
        .as_ref()
        .is_some_and(|job| !job.request().operation.is_pending())
    {
        *pending = None;
    }
    let result = (|| -> Result<()> {
        if pending.is_none() {
            let Some(request) = session
                .field()
                .events
                .world
                .resource_requests
                .first()
                .cloned()
            else {
                return Ok(());
            };
            if session.field_mut().events.resume_resource(&request)? {
                return Ok(());
            }
            let mut assets = session.field_assets().clone();
            let current = session.files();
            let library = current
                .library()
                .context("dynamic resource has no asset library")?
                .clone();
            let diagnostics = resident.diagnostics.clone();
            let id = request.id;
            let task = Task::spawn(move |stop| {
                let cancelled = || stop.load(Ordering::Acquire);
                let mut snapshot = library.snapshot();
                let actors = Actors::capture_resource(&mut snapshot, &mut assets, id, &cancelled)
                    .with_context(|| {
                    format!("field {} script resource {id:#x}", assets.map_id)
                })?;
                let changed = actors.iter().map(|actor| actor.resource).collect();
                for actor in actors {
                    match assets
                        .actors
                        .iter_mut()
                        .find(|old| old.resource == actor.resource)
                    {
                        Some(old) => *old = actor,
                        None => assets.actors.push(actor),
                    }
                }
                let captured = Files::from_snapshot(snapshot, &cancelled, diagnostics)?;
                let mut files = (*current).clone();
                for (path, bytes) in captured.iter() {
                    files.insert(path.clone(), bytes.clone());
                }
                files.disk_bytes += captured.disk_bytes;
                let resources =
                    resonance_game::field::prepare_resource(&assets, |path| files.read(path))?;
                ensure!(!cancelled(), "dynamic resource preparation cancelled");
                Ok(Prepared {
                    assets,
                    files: Arc::new(files),
                    resources,
                    changed,
                })
            })?;
            *pending = Some(Pending::Capturing(request, task));
        }
        if let Some(Pending::Capturing(_, task)) = pending.as_ref() {
            let Some(prepared) = task.poll()? else {
                return Ok(());
            };
            let mut prepared = prepared?;
            let Some(Pending::Capturing(request, _)) = pending.take() else {
                unreachable!()
            };
            prepared.preserve_resident_files(&resident);
            // Publish bytes before AssetServer starts its selected GLB/image loads.
            *resident.files.write().unwrap() = Some(prepared.files.clone());
            for &resource in &prepared.changed {
                let actor = prepared
                    .assets
                    .actors
                    .iter()
                    .find(|actor| actor.resource == resource)
                    .unwrap();
                let parts = actor
                    .parts
                    .iter()
                    .cloned()
                    .map(|part| Part::load(part, &server, &art.loads))
                    .collect();
                art.models.insert(resource, parts);
            }
            art.ready = false;
            art.loading_since = std::time::Instant::now();
            *pending = Some(Pending::Loading(request, prepared));
        }
        if matches!(pending.as_ref(), Some(Pending::Loading(_, _))) && art.ready {
            let Some(Pending::Loading(request, mut prepared)) = pending.take() else {
                unreachable!()
            };
            prepared.preserve_resident_files(&resident);
            ensure!(
                prepared
                    .changed
                    .iter()
                    .all(|id| art.models.contains_key(id)),
                "script resource {:#x} has unavailable model artwork",
                request.id
            );
            session
                .field_mut()
                .events
                .install_resource(&request, prepared.resources)?;
            session.install_field_assets(prepared.assets, prepared.files);
        }
        Ok(())
    })();
    if let Err(error) = result {
        session.field_mut().events.cancel();
        *pending = None;
        failures.fatal("script resource preparation", error);
    }
}
