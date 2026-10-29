use super::*;
use crate::library::{Library, Mod};
use serde_json::{Value, json};
use std::fs;

fn fixture() -> Value {
    json!({"texture":"fonts/test.ktx2", "width":16, "height":8, "line_height":8,
    "glyph_size":[4,8], "advance":4, "glyphs":[
        {"characters":["AB","😀"],"position":[0,0]},
        {"characters":"C","position":[4,0],"size":[2,8],"advance":3}
    ]})
}

#[test]
fn font_groups_share_edits_without_alias_resolution_and_keep_explicit_coverage() -> Result<()> {
    let base = tempfile::tempdir()?;
    let modifications = tempfile::tempdir()?;
    fs::create_dir(base.path().join("fonts"))?;
    fs::create_dir(modifications.path().join("fonts"))?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(&json!({
            "game":"tales-of-symphonia", "version":crate::library::VERSION
        }))?,
    )?;
    fs::write(base.path().join("fonts/test.ktx2"), b"captured atlas")?;
    let path = "fonts/test.json";
    fs::write(base.path().join(path), serde_json::to_vec(&fixture())?)?;
    let library = Library::open(
        base.path(),
        &[Mod {
            name: "font".into(),
            root: modifications.path().into(),
        }],
    )?;
    let mut retained = library.snapshot();
    let original = crate::preparation::font(&mut retained, path)?;
    assert_eq!(original.glyphs[&'A'], original.glyphs[&'😀']);
    assert_eq!(
        original.glyphs[&'C'],
        Glyph {
            rect: [4, 0, 2, 8],
            advance: 3
        }
    );
    assert!(original.validate_text("AB😀C").is_ok());
    assert!(original.validate_text("Z").is_err());
    let mut edited = fixture();
    edited["advance"] = json!(5);
    edited["glyphs"][0]["position"] = json!([8, 0]);
    // Give B its own glyph by removing it from the shared entry.
    edited["glyphs"][0]["characters"] = json!("A😀");
    edited["glyphs"].as_array_mut().unwrap().push(json!({
        "characters":"BZ", "position":[4,0], "advance":2
    }));
    fs::write(
        modifications.path().join(path),
        serde_json::to_vec(&edited)?,
    )?;
    let current = crate::preparation::font(&mut library.snapshot(), path)?;
    assert_eq!(
        current.glyphs[&'A'],
        Glyph {
            rect: [8, 0, 4, 8],
            advance: 5
        }
    );
    assert_eq!(current.glyphs[&'😀'], current.glyphs[&'A']);
    assert_eq!(current.glyphs[&'B'], current.glyphs[&'Z']);
    assert_eq!(current.glyphs[&'B'].advance, 2);
    assert_eq!(current.glyphs[&'C'], original.glyphs[&'C']);
    assert_eq!(
        crate::preparation::font(&mut retained, path)?.glyphs,
        original.glyphs
    );
    let reloaded: BitmapFont = serde_json::from_value(serde_json::to_value(&current)?)?;
    assert_eq!(reloaded.glyphs, current.glyphs);
    Ok(())
}

#[test]
fn font_groups_reject_duplicates_empty_entries_and_the_old_document_shape() -> Result<()> {
    for value in [
        json!({"characters":"AA","position":[0,0]}),
        json!({"characters":["A","A"],"position":[0,0]}),
        json!({"characters":"","position":[0,0]}),
        json!({"characters":[],"position":[0,0]}),
        json!({"characters":"A","position":[0,0],"alias":"B"}),
    ] {
        let mut document = fixture();
        document["glyphs"] = json!([value]);
        assert!(serde_json::from_value::<BitmapFont>(document).is_err());
    }
    let mut duplicate = fixture();
    duplicate["glyphs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"characters":"A","position":[8,0]}));
    assert!(serde_json::from_value::<BitmapFont>(duplicate).is_err());
    let mut versioned = fixture();
    versioned["version"] = json!(1);
    assert!(serde_json::from_value::<BitmapFont>(versioned).is_err());
    let mut old = fixture();
    old["glyphs"] = json!({"A":{"rect":[0,0,4,8],"advance":4}});
    assert!(serde_json::from_value::<BitmapFont>(old).is_err());
    let mut outside = fixture();
    outside["glyphs"][0]["position"] = json!([16, 0]);
    assert!(
        serde_json::from_value::<BitmapFont>(outside)?
            .validate()
            .is_err()
    );
    Ok(())
}
