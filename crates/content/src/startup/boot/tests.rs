use super::*;
use crate::{
    library::{Descriptor, Library, Mod},
    startup,
};
use std::fs;

#[test]
fn startup_defaults_and_invalid_intervals_are_explicit() -> Result<()> {
    let value = serde_json::json!([{"image":"logo.ktx2","fade_out_at":180}]);
    let sequence: Sequence = serde_json::from_value(value.clone())?;
    assert_eq!(sequence.duration()?, 244);
    assert_eq!(serde_json::to_value(&sequence)?, value);
    assert_eq!(Sequence::default().duration()?, 0);
    for changed in [
        serde_json::json!([{"image":"../escape.ktx2","fade_out_at":180}]),
        serde_json::json!([{"image":"logo.ktx2","fade_out_at":180,"fade_ticks":0}]),
        serde_json::json!([{"image":"logo.ktx2","fade_out_at":64}]),
        serde_json::json!([{"image":"logo.ktx2","fade_out_at":180,"skip_after":64}]),
        serde_json::json!([{"image":"logo.ktx2","fade_out_at":180,"skip_after":181}]),
        serde_json::json!([{"image":"logo.ktx2","fade_out_at":4294967295u32}]),
    ] {
        assert!(
            serde_json::from_value::<Sequence>(changed)?
                .duration()
                .is_err()
        );
    }
    assert!(
        serde_json::from_value::<Sequence>(serde_json::json!({"version":1,"textures":[]})).is_err()
    );
    assert!(
        serde_json::from_value::<Sequence>(serde_json::json!([
        {"image":"logo.ktx2","fade_out_at":180,"width":640}]))
        .is_err()
    );
    Ok(())
}

#[test]
fn startup_capture_follows_mod_order_and_can_remove_all_logos() -> Result<()> {
    let work = tempfile::tempdir()?;
    let root = work.path().join("base");
    let modification = work.path().join("mod");
    for path in [&root, &modification] {
        fs::create_dir_all(path.join("ui/boot"))?;
    }
    fs::create_dir_all(root.join("ui/title"))?;
    fs::write(
        root.join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: crate::library::VERSION,
            game: crate::library::GAME.into(),
        })?,
    )?;
    fs::write(
        root.join(startup::PATH),
        serde_json::to_vec(&startup::Definition {
            day_field: 1,
            night_field: 2,
            artwork: vec![],
            music: "music.wav".into(),
            cues: "cues.json".into(),
        })?,
    )?;
    let original = serde_json::json!([
        {"image":"first.ktx2","fade_out_at":180},
        {"image":"second.ktx2","fade_out_at":120}]);
    fs::write(root.join(startup::BOOT), serde_json::to_vec(&original)?)?;
    fs::write(root.join("first.ktx2"), b"first")?;
    fs::write(root.join("second.ktx2"), b"second")?;
    let library = Library::open(&root, &[])?;
    let mut retained = library.snapshot();
    let (_, before) = startup::capture(&mut retained, true, &|| false)?;
    assert_eq!(before.unwrap().duration()?, 428);
    let edited = serde_json::json!([
        {"image":"second.ktx2","fade_out_at":20,"fade_ticks":4,"skip_after":10,"background":[255,255,255]}]);
    fs::write(
        modification.join(startup::BOOT),
        serde_json::to_vec(&edited)?,
    )?;
    let modified = Library::open(
        &root,
        &[Mod {
            name: "logos".into(),
            root: modification.clone(),
        }],
    )?;
    let (_, changed) = startup::capture(&mut modified.snapshot(), true, &|| false)?;
    let changed = changed.unwrap();
    assert_eq!(changed.duration()?, 24);
    assert_eq!(changed.0[0].image, "second.ktx2");
    assert_eq!(changed.0[0].background, [255; 3]);
    fs::remove_file(root.join("first.ktx2"))?;
    assert_eq!(&*retained.read("first.ktx2", 1024)?, b"first");
    assert!(startup::capture(&mut library.snapshot(), true, &|| false).is_err());
    fs::write(modification.join(startup::BOOT), b"[]")?;
    let (_, empty) = startup::capture(&mut modified.snapshot(), true, &|| false)?;
    assert_eq!(empty.unwrap().duration()?, 0);
    fs::remove_file(root.join(startup::BOOT))?;
    assert!(
        startup::capture(&mut library.snapshot(), false, &|| false)?
            .1
            .is_none()
    );
    Ok(())
}
