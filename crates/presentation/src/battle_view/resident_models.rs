//! Startup owns a fixed set of real model bindings. Each mutable live material
//! is leased exclusively; encounter misses cannot expand the pool.
use super::*;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    images: [Option<AssetId<Image>>; 4],
    state: crate::materials::SurfaceKey,
}
impl Key {
    fn new(surface: &TitleSurface) -> Self {
        Self {
            images: [
                &surface.color,
                &surface.sampling,
                &surface.multiply,
                &surface.toon_ramp,
            ]
            .map(|image| image.as_ref().map(Handle::id)),
            state: surface.into(),
        }
    }
}
struct Slot {
    baseline: TitleSurface,
    free: Option<Handle<TitleSurface>>,
}
#[derive(Default)]
struct Pool {
    entries: HashMap<Key, Vec<Slot>>,
    ready: bool,
    #[cfg(test)]
    hits: u64,
}
#[derive(Resource, Default, Clone)]
pub(crate) struct ResidentModels(Arc<Mutex<Pool>>);

pub(super) struct Lease {
    pool: Arc<Mutex<Pool>>,
    key: Key,
    slot: usize,
    handle: Handle<TitleSurface>,
}
impl Lease {
    pub fn reset(&self, surfaces: &mut Assets<TitleSurface>) {
        let pool = self.pool.lock().unwrap();
        let baseline = &pool.entries[&self.key][self.slot].baseline;
        if surfaces.get(&self.handle) != Some(baseline) {
            if let Some(mut surface) = surfaces.get_mut(&self.handle) {
                *surface = baseline.clone();
            }
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut pool = self.pool.lock().unwrap();
        let slot = &mut pool.entries.get_mut(&self.key).unwrap()[self.slot];
        debug_assert!(slot.free.is_none());
        slot.free = Some(self.handle.clone());
    }
}

impl ResidentModels {
    /// Only startup can add slots. One binding per exact recipe is enough for
    /// its first live instance; simultaneous copies use encounter-owned assets.
    pub(super) fn warm(
        &self,
        recipe: TitleSurface,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Option<Handle<TitleSurface>> {
        let mut pool = self.0.lock().unwrap();
        debug_assert!(!pool.ready);
        let slots = pool.entries.entry(Key::new(&recipe)).or_default();
        if slots.iter().any(|slot| slot.baseline == recipe) {
            return None;
        }
        let handle = surfaces.add(recipe.clone());
        slots.push(Slot {
            baseline: recipe,
            free: Some(handle.clone()),
        });
        Some(handle)
    }

    pub(super) fn acquire(
        &self,
        recipe: &TitleSurface,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Option<(Handle<TitleSurface>, Lease)> {
        let key = Key::new(recipe);
        let mut pool = self.0.lock().unwrap();
        if !pool.ready {
            return None;
        }
        let (slot, handle) = pool
            .entries
            .get_mut(&key)?
            .iter_mut()
            .enumerate()
            .find_map(|(index, slot)| {
                (slot.baseline == *recipe)
                    .then(|| slot.free.take().map(|handle| (index, handle)))
                    .flatten()
            })?;
        #[cfg(test)]
        {
            pool.hits += 1;
        }
        drop(pool);
        let lease = Lease {
            pool: self.0.clone(),
            key,
            slot,
            handle: handle.clone(),
        };
        // Normal retirement resets while the field is on screen. This also
        // handles failed preparations whose leases were released by Drop.
        lease.reset(surfaces);
        Some((handle, lease))
    }

    pub(crate) fn finish_startup(&self) {
        self.0.lock().unwrap().ready = true;
    }
    pub(super) fn discard_startup(&self) {
        self.0.lock().unwrap().entries.clear();
    }
    pub(crate) fn len(&self) -> usize {
        self.0.lock().unwrap().entries.values().map(Vec::len).sum()
    }
    #[cfg(test)]
    pub(crate) fn hits(&self) -> u64 {
        self.0.lock().unwrap().hits
    }
    #[cfg(test)]
    pub(crate) fn check_returned(&self, surfaces: &Assets<TitleSurface>) -> Result<()> {
        let pool = self.0.lock().unwrap();
        for slot in pool.entries.values().flatten() {
            let handle = slot
                .free
                .as_ref()
                .context("resident model binding still leased")?;
            ensure!(
                surfaces.get(handle) == Some(&slot.baseline),
                "resident model appearance was not reset"
            );
        }
        Ok(())
    }

    /// Audit the owning pool independently of the startup draw report. Most
    /// resident materials deliberately have no temporary warm draw entity.
    #[cfg(test)]
    pub(crate) fn install_upload_audit(app: &mut App) {
        use bevy::render::{Render, RenderApp, RenderSystems};
        app.sub_app_mut(RenderApp).add_systems(
            Render,
            (|pool: Res<Self>, observer: gpu::Observer, mut checked: Local<bool>| {
                if !*checked && pool.0.lock().unwrap().ready {
                    pool.check_uploaded(&observer.materials, &observer.allocators)
                        .expect("common model binding audit");
                    *checked = true;
                }
            })
            .in_set(RenderSystems::Cleanup),
        );
    }

    #[cfg(test)]
    fn check_uploaded(
        &self,
        materials: &bevy::render::erased_render_asset::ErasedRenderAssets<
            bevy::pbr::PreparedMaterial,
        >,
        allocators: &bevy::render::material_bind_groups::MaterialBindGroupAllocators,
    ) -> Result<()> {
        let allocator = allocators.get(&std::any::TypeId::of::<TitleSurface>());
        let pool = self.0.lock().unwrap();
        for slot in pool.entries.values().flatten() {
            let handle = slot
                .free
                .as_ref()
                .context("resident material still leased")?;
            let material = materials
                .get(handle.id())
                .with_context(|| format!("resident material {:?} not uploaded", handle.id()))?;
            ensure!(
                allocator
                    .and_then(|a| a.get(material.binding.group))
                    .and_then(|slab| slab.bind_group())
                    .is_some(),
                "resident material {:?} has no GPU bind group",
                handle.id()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_are_exclusive_and_reset_all_mutable_appearance_state() {
        let resident = ResidentModels::default();
        let mut surfaces = Assets::default();
        let baseline = TitleSurface::default();
        let original = resident.warm(baseline.clone(), &mut surfaces).unwrap();
        assert!(resident.warm(baseline.clone(), &mut surfaces).is_none());
        assert!(resident.acquire(&baseline, &mut surfaces).is_none());
        resident.finish_startup();
        let (handle, lease) = resident.acquire(&baseline, &mut surfaces).unwrap();
        assert_eq!(handle, original);
        assert!(resident.acquire(&baseline, &mut surfaces).is_none());
        let mut surface = surfaces.get_mut(&handle).unwrap();
        surface.tint = Vec4::ZERO;
        surface.uv_offsets = Vec4::ONE;
        surface.fog_range = Vec4::ONE;
        surface.field_light = Vec4::ONE;
        surface.ambient_scale = Vec3::ZERO;
        drop(surface);
        lease.reset(&mut surfaces);
        assert_eq!(surfaces.get(&handle), Some(&baseline));
        // A failed preparation can release through Drop without a reset.
        surfaces.get_mut(&handle).unwrap().red_channel = true;
        drop(lease);
        let (reused, lease) = resident.acquire(&baseline, &mut surfaces).unwrap();
        assert_eq!(reused, original);
        assert_eq!(surfaces.get(&reused), Some(&baseline));
        let changed = TitleSurface {
            uv_offsets: Vec4::ONE,
            ..baseline
        };
        assert!(resident.acquire(&changed, &mut surfaces).is_none());
        assert_eq!(resident.len(), 1, "encounters cannot add slots");
        drop(lease);
    }
}
