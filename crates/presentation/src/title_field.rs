//! Load the title background using the host's local time.
use super::camera;
use bevy::prelude::{PerspectiveProjection, Projection, Resource};

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

pub(super) fn load(
    root: &std::path::Path,
) -> anyhow::Result<(Scene, resonance_events::EventRuntime)> {
    use anyhow::{Context, ensure};
    use resonance_content::{
        effect::{FieldEffects, sprite::ORB_SPRITE},
        prepared::{Cache, Files},
    };
    let map = match time::OffsetDateTime::now_local() {
        Ok(now) if (7..19).contains(&now.hour()) => 474, // Day: 07:00–19:00.
        Ok(_) => 531,                                    // Night.
        Err(error) => {
            eprintln!("Cannot read the host's local time ({error}); using the day title field");
            474
        }
    };
    let files = Files::load(
        root,
        &[&resonance_content::field::preload_path(map)],
        &mut Cache::default(),
        || false,
    )
    .context("prepare title field; rerun resonance-import cook-all if assets are outdated")?;
    let mut assets = files.field(map)?;
    // The title has no playable party. Its script creates the embedded field actors.
    assets
        .actors
        .retain(|actor| resonance_content::field::LOCAL_MODEL_RESOURCES.contains(&actor.resource));
    ensure!(
        !assets.texture_animations.is_empty(),
        "title field is missing native UV controllers; rerun resonance-import cook-all"
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
    Ok((Scene { assets, glow }, events))
}
