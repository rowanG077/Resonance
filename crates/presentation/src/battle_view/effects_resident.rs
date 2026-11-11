//! Only startup's fixed common set owns bindings. Encounters cannot grow it.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    color: Option<AssetId<Image>>,
    alpha: Option<AssetId<Image>>,
    state: MaterialKey,
}

impl Key {
    pub(super) fn new(
        mut state: MaterialKey,
        color: Option<&Handle<Image>>,
        alpha: Option<&Handle<Image>>,
    ) -> Self {
        // Encounter resource IDs and atlas indices are local. Actual texture
        // identity and immutable render state determine a reusable binding.
        state.atlas = None;
        Self {
            color: color.map(Handle::id),
            alpha: alpha.map(Handle::id),
            state,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct ResidentBindings {
    bindings: BTreeMap<Key, Handle<TitleSurface>>,
    ready: bool,
    #[cfg(test)]
    hits: std::sync::atomic::AtomicU64,
    #[cfg(test)]
    textured_hits: std::sync::atomic::AtomicU64,
}

impl ResidentBindings {
    pub(super) fn get(&self, key: &Key, warming: bool) -> Option<Handle<TitleSurface>> {
        if !self.ready && !warming {
            return None;
        }
        let binding = self.bindings.get(key)?;
        #[cfg(test)]
        self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        if key.color.is_some() {
            self.textured_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Some(binding.clone())
    }

    pub(super) fn retain(&mut self, key: Key, surface: Handle<TitleSurface>) {
        self.bindings.entry(key).or_insert(surface);
    }

    pub(crate) fn handles(&self) -> impl Iterator<Item = &Handle<TitleSurface>> {
        self.bindings.values()
    }

    pub(crate) fn len(&self) -> usize {
        self.bindings.len()
    }

    pub(crate) fn finish_startup(&mut self) {
        self.ready = true;
    }

    pub(crate) fn discard_startup(&mut self) {
        self.bindings.clear();
    }

    #[cfg(test)]
    pub(crate) fn hits(&self) -> u64 {
        self.hits.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn textured_hits(&self) -> u64 {
        self.textured_hits
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn check_retained(world: &World) -> Result<()> {
        let Some(resident) = world.get_resource::<Self>() else {
            return Ok(());
        };
        let surfaces = world.resource::<Assets<TitleSurface>>();
        let images = world.resource::<Assets<Image>>();
        for (key, handle) in &resident.bindings {
            let surface = surfaces
                .get(handle)
                .context("resident effect material was released")?;
            ensure!(
                key.color
                    .into_iter()
                    .chain(key.alpha)
                    .all(|id| images.contains(id)),
                "resident effect source image was released"
            );
            ensure!(
                surface
                    .color
                    .iter()
                    .chain(&surface.sampling)
                    .chain(&surface.multiply)
                    .all(|image| images.contains(image)),
                "resident effect sampler view was released"
            );
        }
        Ok(())
    }
}
