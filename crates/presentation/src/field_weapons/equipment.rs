//! Session-owned equipment assets and their explicit content preparation.
use crate::{
    field_view::{Failures, Part},
    loading::{Resident, Task},
};
use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use resonance_content::{
    battle_model::{Attachment, AttachmentTextureAnimation},
    model_preview::PreviewPart,
    prepared::Files,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

#[derive(Resource, Default)]
pub(crate) struct EquipmentCache {
    pub(super) equipment: BTreeMap<u16, Equipment>,
    pub(super) failed: BTreeSet<u16>,
    pending: Option<(u16, Task<Captured>)>,
    loads: crate::loading::LoadTasks,
    files: BTreeMap<String, Arc<[u8]>>,
    published: Option<std::sync::Weak<Files>>,
}

impl EquipmentCache {
    pub(super) fn assets_ready(&self, world: &resonance_events::GameWorld) -> bool {
        equipped(world).all(|item| {
            self.failed.contains(&item)
                || self.equipment.get(&item).is_some_and(|model| model.ready)
        })
    }

    fn publish_files(
        &mut self,
        resident: &Resident,
        live: &mut Option<ResMut<crate::new_game::Session>>,
    ) -> Result<()> {
        let current = resident
            .files
            .read()
            .unwrap()
            .clone()
            .context("field weapons have no resident files")?;
        if self
            .published
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|files| Arc::ptr_eq(&files, &current))
        {
            return Ok(());
        }
        let mut files = current;
        if self.files.keys().any(|path| !files.contains_key(path)) {
            let files = Arc::make_mut(&mut files);
            for (path, bytes) in &self.files {
                if !files.contains_key(path) {
                    files.disk_bytes += bytes.len() as u64;
                    files.insert(path.clone(), bytes.clone());
                }
            }
        }
        *resident.files.write().unwrap() = Some(files.clone());
        if let Some(session) = live.as_mut().filter(|s| s.is_field()) {
            let assets = session.field_assets().clone();
            session.install_field_assets(assets, files.clone());
        }
        self.published = Some(Arc::downgrade(&files));
        Ok(())
    }
}

fn equipped(world: &resonance_events::GameWorld) -> impl Iterator<Item = u16> + '_ {
    world.party.iter().flat_map(|party| {
        party
            .members
            .iter()
            .enumerate()
            .flat_map(|(index, member)| member_equipment(index, member))
            .filter(|item| *item != 0)
    })
}

pub(super) struct Equipment {
    pub(super) shield: bool,
    pub(super) layers: Vec<Layer>,
    pub(super) ready: bool,
    pub(super) started: Instant,
}

pub(super) struct Layer {
    pub(super) part: u8,
    pub(super) preview: PreviewPart,
    pub(super) animation: Option<AttachmentTextureAnimation>,
    pub(super) art: Part,
}

pub(super) struct Captured {
    pub(super) shield: bool,
    pub(super) layers: Vec<(u8, PreviewPart, Option<AttachmentTextureAnimation>)>,
    files: Files,
}

pub(super) fn capture(
    library: &resonance_content::library::Library,
    item: u16,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Captured> {
    let mut snapshot = library.snapshot();
    let attachment = resonance_content::battle_model::document::capture_equipment(
        &mut snapshot,
        item,
        diagnostics,
        cancelled,
    )?;
    let shield = matches!(attachment, Attachment::Shield(_));
    let mut layers = Vec::new();
    if let Attachment::Weapon(weapon) | Attachment::Shield(weapon) = attachment {
        for (slot, model) in weapon.parts {
            let controls = weapon.uv_animation.get(&slot);
            for mut preview in model.layers {
                // Kendamas carry combat-linked motion banks rather than an
                // autoplay selection. Field scenes use their authored idle.
                if preview.animation.is_none()
                    && preview
                        .scene
                        .clips
                        .iter()
                        .any(|clip| clip.resource_slot == 60)
                {
                    preview.animation = Some(60);
                }
                preview.selected_clip()?;
                let independent = model.layer_skeletons.contains_key(&preview.scene.resource);
                let animation = controls.and_then(|controls| {
                    if independent {
                        &controls.independent
                    } else {
                        &controls.primary
                    }
                    .clone()
                });
                if let Some(animation) = &animation {
                    validate_texture_animation(animation, preview.scene.textures.len())?;
                }
                layers.push((slot, preview, animation));
            }
        }
    }
    Ok(Captured {
        shield,
        layers,
        files: Files::from_snapshot(snapshot, cancelled, diagnostics.clone())?,
    })
}

fn validate_texture_animation(
    animation: &AttachmentTextureAnimation,
    textures: usize,
) -> Result<()> {
    let texture = match animation {
        AttachmentTextureAnimation::Scroll {
            texture,
            step_milli_uv,
            offset_milli_uv,
        } => {
            ensure!(
                step_milli_uv
                    .iter()
                    .all(|step| (-1000..=1000).contains(step))
                    && offset_milli_uv
                        .iter()
                        .all(|offset| (-999..=999).contains(offset)),
                "invalid field weapon texture scroll"
            );
            *texture
        }
        AttachmentTextureAnimation::Atlas {
            texture,
            rows,
            frames,
            ..
        } => {
            ensure!(
                *rows > 0 && frames.iter().all(|frame| frame < rows),
                "invalid field weapon texture atlas"
            );
            *texture
        }
    };
    ensure!(
        usize::from(texture) < textures,
        "field weapon texture animation binding is missing"
    );
    Ok(())
}

fn member_equipment(index: usize, member: &resonance_events::party::Member) -> [u16; 2] {
    // The other characters use this equipment slot for non-carried armour.
    [
        member.equipment[0],
        if matches!(index, 5 | 8) {
            member.equipment[5]
        } else {
            0
        },
    ]
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    mut cache: ResMut<EquipmentCache>,
    mut live: Option<ResMut<crate::new_game::Session>>,
    checkpoint: Option<Res<crate::field_view::Session>>,
    resident: Res<Resident>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    clips: Res<Assets<crate::sparse_animation::Clip>>,
    images: Res<Assets<Image>>,
    mut failures: Failures,
) {
    if resident.battle.load(Ordering::Acquire) {
        return;
    }
    let result = (|| -> Result<()> {
        cache.publish_files(&resident, &mut live)?;
        if let Some((item, task)) = &cache.pending
            && let Some(result) = task.poll()?
        {
            let item = *item;
            cache.pending = None;
            match result {
                Ok(captured) => {
                    for (path, bytes) in captured.files.iter() {
                        cache.files.insert(path.clone(), bytes.clone());
                    }
                    cache.published = None;
                    cache.publish_files(&resident, &mut live)?;
                    let layers = captured
                        .layers
                        .into_iter()
                        .map(|(part, preview, animation)| {
                            let artwork = Part::load(preview.scene.clone(), &server, &cache.loads);
                            Layer {
                                part,
                                preview,
                                animation,
                                art: artwork,
                            }
                        })
                        .collect();
                    cache.equipment.insert(
                        item,
                        Equipment {
                            shield: captured.shield,
                            layers,
                            ready: false,
                            started: Instant::now(),
                        },
                    );
                }
                Err(error) => {
                    cache.failed.insert(item);
                    failures.skip(
                        "field equipment capture",
                        error.context(format!("item {item}")),
                    );
                }
            }
        }
        let mut failed = Vec::new();
        for (&item, equipment) in &mut cache.equipment {
            if equipment.ready {
                continue;
            }
            let resolved = equipment.layers.iter_mut().try_fold(true, |ready, layer| {
                if let Some(error) = layer.art.load_error(&server) {
                    anyhow::bail!("{error}");
                }
                Ok::<_, anyhow::Error>(
                    layer.art.resolve(&server, &gltfs, &clips, &images)? && ready,
                )
            });
            match resolved {
                Ok(ready) if ready || equipment.started.elapsed().as_secs() < 30 => {
                    equipment.ready = ready
                }
                Ok(_) => failed.push((item, anyhow::anyhow!("equipment {item} load timed out"))),
                Err(error) => failed.push((item, error)),
            }
        }
        for (item, error) in failed {
            cache.equipment.remove(&item);
            cache.failed.insert(item);
            failures.skip("field equipment preparation", error);
        }
        let field = if let Some(checkpoint) = &checkpoint {
            &checkpoint.0
        } else if let Some(session) = live.as_ref().filter(|s| s.is_field()) {
            session.field()
        } else {
            return Ok(());
        };
        let world = &field.events.world;
        if world.party.is_none() {
            return Ok(());
        }
        // Prepare the whole roster before presentation, including characters
        // that the next cutscene has not created yet.
        let wanted: BTreeSet<_> = equipped(world).collect();
        if cache.pending.is_some() {
            return Ok(());
        }
        let Some(item) = wanted
            .iter()
            .copied()
            .find(|item| !cache.equipment.contains_key(item) && !cache.failed.contains(item))
        else {
            return Ok(());
        };
        let files = resident.files.read().unwrap().clone();
        let Some(library) = files.as_ref().and_then(|files| files.library()).cloned() else {
            return Ok(());
        };
        let diagnostics = resident.diagnostics.clone();
        cache.pending = Some((
            item,
            Task::spawn(move |stop| {
                capture(&library, item, &diagnostics, &|| {
                    stop.load(Ordering::Acquire)
                })
            })?,
        ));
        Ok(())
    })();
    if let Err(error) = result {
        failures.fatal("field weapons", error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_bytes_are_republished_after_field_residency_changes() -> Result<()> {
        let resident = Resident::default();
        *resident.files.write().unwrap() = Some(Arc::new(Files::default()));
        let bytes: Arc<[u8]> = Arc::from([1, 2, 3]);
        let mut cache = EquipmentCache::default();
        cache.files.insert("weapon.glb".into(), bytes.clone());
        cache.publish_files(&resident, &mut None)?;
        let first = resident.files.read().unwrap().clone().unwrap();
        *resident.files.write().unwrap() = Some(Arc::new(Files::default()));
        cache.publish_files(&resident, &mut None)?;
        let second = resident.files.read().unwrap().clone().unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&bytes, &second.read("weapon.glb")?));
        cache.publish_files(&resident, &mut None)?;
        assert!(Arc::ptr_eq(
            &second,
            resident.files.read().unwrap().as_ref().unwrap()
        ));
        Ok(())
    }
}
