use super::*;
use crate::library::{Descriptor, GAME, Library, VERSION};
use serde_json::json;
use std::fs;

fn image(size: u32) -> Result<Vec<u8>> {
    Ok(resonance_asset_writer::ktx2::encode_rgba8(
        size,
        size,
        &vec![255; (size * size * 4) as usize],
    )?)
}
fn document() -> serde_json::Value {
    json!({"mappings":{"851968":"lloyd/neutral","851969":"lloyd/neutral"},
    "images":{"lloyd/neutral":["skits/portraits/lloyd/neutral.ktx2"]},
    "animations":{
        "still":{"ids":[12,13]},
        "blink":{"ids":[0,21],"eyes":{"repeat":true,"frames":[{"duration_ticks":181,"image":0}]}}
    }})
}
#[test]
fn portrait_dimensions_shared_selectors_and_mod_edits_follow_current_files() -> Result<()> {
    let base = tempfile::tempdir()?;
    let modification = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: VERSION,
            game: GAME.into(),
        })?,
    )?;
    let path = "skits/portraits/lloyd/neutral.ktx2";
    fs::create_dir_all(base.path().join(path).parent().unwrap())?;
    fs::write(base.path().join(path), image(16)?)?;
    fs::write(base.path().join(PATH), serde_json::to_vec(&document())?)?;
    let library = Library::open_paths(base.path(), &[modification.path().to_owned()])?;
    let mut retained = library.snapshot();
    let before: Portraits = retained.json(PATH, 10000)?;
    let (images, recipes) = before.resolve(|p, l| retained.read_optional(p, l))?;
    assert_eq!(images[&851968].size, [16, 16]);
    assert_eq!(images[&851969].size, [16, 16]);
    assert_eq!(recipes[&0], recipes[&21]);
    assert_eq!(recipes[&0].tracks[0][0].ticks, 180);
    assert_eq!(recipes[&12], PortraitRecipe::default());
    assert!(!recipes.contains_key(&1));
    let mut edited = document();
    edited["animations"]["blink"]["eyes"]["frames"][0]["duration_ticks"] = json!(9);
    fs::create_dir_all(modification.path().join(path).parent().unwrap())?;
    fs::write(modification.path().join(path), image(24)?)?;
    fs::write(modification.path().join(PATH), serde_json::to_vec(&edited)?)?;
    let mut fresh = library.snapshot();
    let changed: Portraits = fresh.json(PATH, 10000)?;
    let (images, recipes) = changed.resolve(|p, l| fresh.read_optional(p, l))?;
    assert_eq!(images[&851968].size, [24, 24]);
    assert_eq!(images[&851968].images[0].size, [24, 24]);
    assert_eq!(recipes[&0].tracks[0][0].ticks, 8);
    assert_eq!(recipes[&21].tracks[0][0].ticks, 8);
    let (images, recipes) = before.resolve(|p, l| retained.read_optional(p, l))?;
    assert_eq!(images[&851968].size, [16, 16]);
    assert_eq!(recipes[&0].tracks[0][0].ticks, 180);
    Ok(())
}
#[test]
fn portrait_tracks_reject_conflicting_selectors_and_invalid_durations() -> Result<()> {
    for value in [
        json!({"mappings":{},"images":{},"animations":{"first":{"ids":[1]},"second":{"ids":[1]}}}),
        json!({"mappings":{},"images":{},"animations":{"first":{"ids":[1],"eyes":{"frames":[]}}}}),
        json!({"mappings":{},"images":{},"animations":{"first":{"ids":[1],"eyes":{"frames":[{"duration_ticks":0}]}}}}),
        json!({"mappings":{"1":"missing"},"images":{},"animations":{}}),
    ] {
        assert!(
            serde_json::from_value::<Portraits>(value)?
                .resolve(|_, _| Ok(None))
                .is_err()
        );
    }
    let too_long = serde_json::json!({"ids":[1],"eyes":{"frames":vec![serde_json::json!({"duration_ticks":1});256]}});
    assert!(
        serde_json::from_value::<Animation>(too_long)?
            .recipe()
            .is_err()
    );
    assert!(
        serde_json::from_value::<Portraits>(
            json!({"mappings":{},"images":{},"animations":{"blink":{"ids":[1],"mouthh":{}}}})
        )
        .is_err()
    );
    Ok(())
}
