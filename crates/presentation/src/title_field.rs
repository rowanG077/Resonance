//! Load the title background using the host's local time.
use super::camera;
use bevy::prelude::{PerspectiveProjection, Projection, Resource};
use resonance_content::{diagnostics::Diagnostics, library::Library, prepared::Files};
use std::sync::Arc;
#[cfg(test)]
mod tests;

pub(super) fn projection(aspect_ratio: f32) -> Projection {
    Projection::custom(camera::TitleProjection(PerspectiveProjection {
        fov: 27_f32.to_radians(),
        aspect_ratio,
        near: camera::FIELD_NEAR,
        far: camera::FIELD_FAR,
        ..Default::default()
    }))
}

#[derive(Resource)]
pub(super) struct Scene {
    pub assets: resonance_content::field::FieldAssets,
    pub glow: resonance_content::effect::SpriteRecipe,
}

pub(super) struct Prepared {
    pub scene: Scene,
    pub events: resonance_events::EventRuntime,
    pub art: resonance_content::TitleAssets,
    pub audio: super::audio::PlaybackAssets,
    pub boot: Option<resonance_content::BootAssets>,
    pub files: Arc<Files>,
}

pub(super) fn prepare(
    library: &Library,
    boot: bool,
    diagnostics: Diagnostics,
) -> anyhow::Result<Prepared> {
    let hour = match time::OffsetDateTime::now_local() {
        Ok(now) => now.hour(),
        Err(error) => {
            eprintln!("Cannot read the host's local time ({error}); using the day title field");
            12
        }
    };
    capture(library, boot, hour, diagnostics, || false)
}

fn capture(
    library: &Library,
    boot: bool,
    hour: u8,
    diagnostics: Diagnostics,
    cancelled: impl Fn() -> bool,
) -> anyhow::Result<Prepared> {
    use anyhow::{Context, ensure};
    use resonance_content::{
        effect::{FieldEffects, sprite::ORB_SPRITE},
        field::document,
        preparation,
    };
    let mut snapshot = library.snapshot();
    let (definition, art, boot) =
        resonance_content::startup::capture(&mut snapshot, boot, &cancelled)?;
    let map = if (7..19).contains(&hour) {
        definition.day_field
    } else {
        definition.night_field
    };
    let (_, mut assets) = document::capture_scene(&mut snapshot, map, &cancelled)?;
    preparation::ui(&mut snapshot, &diagnostics, &cancelled)?;
    let audio = super::audio::PlaybackAssets::capture(
        &mut snapshot,
        &definition,
        diagnostics.clone(),
        &cancelled,
    )?;
    let files = Arc::new(Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        &cancelled,
        diagnostics,
    )?);
    // The title has no playable party. Its script creates the embedded field actors.
    assets
        .actors
        .retain(|actor| resonance_content::field::LOCAL_MODEL_RESOURCES.contains(&actor.resource));
    ensure!(
        !assets.texture_animations.is_empty(),
        "title field {map} is missing native UV controllers"
    );
    let effects: FieldEffects = files.json(&assets.effects)?;
    effects.validate()?;
    let glow = effects
        .sprites
        .get(&ORB_SPRITE)
        .context("missing title glow sprite")?
        .clone();
    let events =
        resonance_game::title_events::start(&files.read(&assets.script)?, &assets, |path| {
            files.read(path)
        })?;
    ensure!(!cancelled(), "title preparation cancelled");
    Ok(Prepared {
        scene: Scene { assets, glow },
        events,
        art,
        audio,
        boot,
        files,
    })
}
