use super::*;
use crate::library::{Descriptor, Library, Mod};
use std::fs;

#[test]
fn title_uses_explicit_artwork_and_retains_mod_snapshots() -> Result<()> {
    let work = tempfile::tempdir()?;
    let root = work.path().join("base");
    let modification = work.path().join("mod");
    for directory in [&root, &modification] {
        fs::create_dir_all(directory.join("ui/title"))?;
    }
    fs::write(
        root.join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: crate::library::VERSION,
            game: crate::library::GAME.into(),
        })?,
    )?;
    // The consumer binds behavior to names, not to a source array index. One
    // drawing entry is a valid mod; unused art and skipped boot need not exist.
    let mut definition: Definition = serde_json::from_value(serde_json::json!({
        "day_field":1,"night_field":2,"music":"music.wav","cues":"cues.json",
        "artwork":[{"image":"normal.ktx2","selected_image":"selected.ktx2",
            "choice":"continue","top":100}]
    }))?;
    fs::write(root.join(PATH), serde_json::to_vec(&definition)?)?;
    fs::write(root.join("normal.ktx2"), b"normal")?;
    fs::write(root.join("selected.ktx2"), b"selected")?;
    let library = Library::open(&root, &[])?;
    let mut retained = library.snapshot();
    let (title, boot) = capture(&mut retained, false, &|| false)?;
    let art = title.artwork;
    assert!(boot.is_none());
    assert_eq!(art.len(), 1);
    assert_eq!(art[0].choice.unwrap().row(), 1);
    assert_eq!(&*retained.read("normal.ktx2", 1024)?, b"normal");

    definition.artwork[0].image = "replacement.ktx2".into();
    definition.artwork[0].top = 300.;
    definition.artwork[0].choice = Some(Choice::NewGame);
    fs::write(modification.join(PATH), serde_json::to_vec(&definition)?)?;
    fs::write(modification.join("replacement.ktx2"), b"replacement")?;
    let modified = Library::open(
        &root,
        &[Mod {
            name: "title".into(),
            root: modification,
        }],
    )?;
    let mut snapshot = modified.snapshot();
    let (changed, _) = capture(&mut snapshot, false, &|| false)?;
    let changed = changed.artwork;
    assert_eq!(changed[0].top, 300.);
    assert_eq!(changed[0].choice.unwrap().row(), 0);
    assert_eq!(&*snapshot.read("replacement.ktx2", 1024)?, b"replacement");
    assert_eq!(art[0].top, 100.);
    assert_eq!(art[0].image, "normal.ktx2");
    fs::remove_file(root.join("selected.ktx2"))?;
    assert!(capture(&mut library.snapshot(), false, &|| false).is_err());
    assert_eq!(&*retained.read("selected.ktx2", 1024)?, b"selected");
    assert!(capture(&mut retained, false, &|| true).is_err());
    Ok(())
}

#[test]
fn title_rejects_ambiguous_or_unsafe_artwork_without_a_legacy_reader() -> Result<()> {
    let value = serde_json::json!({"day_field":1,"night_field":2,
        "music":"music.wav","cues":"cues.json",
        "artwork":[{"image":"logo.ktx2","top":24}]});
    let mut definition: Definition = serde_json::from_value(value.clone())?;
    definition.validate()?;
    definition.artwork[0].selected_image = Some("selected.ktx2".into());
    assert!(definition.validate().is_err());
    definition.artwork[0].choice = Some(Choice::Customize);
    definition.validate()?;
    definition.artwork[0].selected_image = Some("../outside.ktx2".into());
    assert!(definition.validate().is_err());
    let mut old = value.clone();
    old["version"] = 1.into();
    assert!(serde_json::from_value::<Definition>(old).is_err());
    let mut unknown = value;
    unknown["artwork"][0]["index"] = 2.into();
    assert!(serde_json::from_value::<Definition>(unknown).is_err());
    Ok(())
}
