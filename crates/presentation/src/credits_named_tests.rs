use super::*;
use resonance_content::{
    credits::document,
    diagnostics::Diagnostics,
    game_over,
    library::{Library, Mod},
    prepared::Files,
};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

fn write(root: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, bytes)?;
    Ok(())
}
fn json(root: &Path, path: &str, value: serde_json::Value) -> Result<()> {
    write(root, path, &serde_json::to_vec_pretty(&value)?)
}
fn wave(rate: u32, frames: u32) -> Result<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..frames {
        writer.write_sample(2000i16)?;
        writer.write_sample(-1000i16)?;
    }
    writer.finalize()?;
    Ok(bytes.into_inner())
}
fn picture(size: u32) -> Result<Vec<u8>> {
    Ok(resonance_asset_writer::ktx2::encode_rgba8(
        size,
        size,
        &vec![255; (size * size * 4) as usize],
    )?)
}
fn script(expression: &str) -> Result<Vec<u8>> {
    Ok(symphonia_script::scenario::assemble(&format!(
        ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n{expression}\ncalc 0\narg\npush.s8 0\ncalc 0\narg\nproc 0x6d\nend\n"
    ))?)
}
fn fixture() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    let p = root.path();
    write(
        p,
        "content.json",
        br#"{"version":94,"game":"tales-of-symphonia"}"#,
    )?;
    let characters: String = (32u8..=126).map(char::from).collect();
    json(
        p,
        "fonts/dialogue.json",
        serde_json::json!({"texture":"fonts/dialogue.ktx2",
        "width":1,"height":1,"line_height":1,"glyph_size":[1,1],"advance":1,
        "glyphs":[{"characters":characters,"position":[0,0]}]}),
    )?;
    write(p, "fonts/dialogue.ktx2", b"opaque atlas bytes")?;
    let texture = "ui/unused.ktx2";
    json(
        p,
        "ui/dialogue.json",
        serde_json::json!({"font":"fonts/dialogue.json",
        "symbols":texture,"shading":texture,"patterns":vec![texture;5],
        "lined_fill":texture,"plain_fill":texture,"cursor":texture,"default_style":"plain",
        "selection":{"color":[255,255,255,255],"row_offsets":vec![0;9],"bob_amplitude":0,"bob_radians_per_tick":0}}),
    )?;
    json(
        p,
        game_over::document::PATH,
        serde_json::json!({
        "background":"ui/game-over/background.ktx2",
        "font":"fonts/dialogue.json","caption":"Try again", "choices":{"load":"Load","title":"Quit"}}),
    )?;
    write(
        p,
        "ui/game-over/background.ktx2",
        &resonance_asset_writer::ktx2::encode_rgba8(640, 480, &vec![255; 640 * 480 * 4])?,
    )?;
    json(
        p,
        document::PATH,
        serde_json::json!({
        "style":{"glyph_size":[16,16],"color":[255,255,255,255],"line_height":20,"tab_width":32},
        "scroll":{"height_pixels":1000,"speed_divisor_ticks":100,"stop_margin_pixels":100},
        "blocks":[{"lines":["Credits"],"align":"left"},{"image":"ui/credits/team.ktx2"}],
        "music":{"path":"audio/music/credits.wav"},"final_hold_ticks":180}),
    )?;
    write(p, "ui/credits/team.ktx2", &picture(16)?)?;
    write(p, "audio/music/credits.wav", &wave(32000, 320)?)?;
    write(p, "fields/ending/scenario.ssb", &script("push.s8 18")?)?;
    Ok(root)
}
fn prepare(library: &Library, diagnostics: Diagnostics) -> Result<Files> {
    let mut snapshot = library.snapshot();
    resonance_game::screen_resources::capture(
        &mut snapshot,
        ["fields/ending/scenario.ssb"],
        &diagnostics,
        &|| false,
    )?;
    Files::from_snapshot(snapshot, || false, diagnostics)
}
fn audio(manifest: &Manifest, files: &Files) -> Result<Audio> {
    Ok(Audio {
        clip: Arc::new(crate::field_audio::Clip::prepare(
            files.read(&manifest.music.path)?,
            &manifest.music,
        )?),
        rate: manifest.music.sample_rate,
        gain: 1.,
        mono: false,
    })
}

#[test]
fn named_screens_capture_mod_text_art_and_current_recording_clocks() -> Result<()> {
    let root = fixture()?;
    let base = Library::open(root.path(), &[])?;
    let first = prepare(&base, Diagnostics::new(true))?;
    let original = Manifest::load(&first)?;
    assert_eq!(
        (original.music.frames, original.music.sample_rate),
        (320, 32000)
    );
    assert_eq!(
        resonance_game::game_over::Assets::from_prepared(first.clone())?
            .art
            .caption,
        "Try again"
    );
    let modification = tempfile::tempdir()?;
    write(
        modification.path(),
        "audio/music/credits.wav",
        &wave(16000, 1600)?,
    )?;
    write(modification.path(), "ui/credits/team.ktx2", &picture(24)?)?;
    let mut credit_text: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join(document::PATH))?)?;
    credit_text["blocks"][0] =
        serde_json::json!({"lines":["Edited credits", "Mod author"],"line_spacing":36});
    json(modification.path(), document::PATH, credit_text)?;
    let mut game_over: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join(game_over::document::PATH))?)?;
    game_over["caption"] = "A different ending".into();
    game_over["choices"] = serde_json::json!({"load":"Resume", "title":"Return"});
    json(modification.path(), game_over::document::PATH, game_over)?;
    let modified = Library::open(
        root.path(),
        &[Mod {
            name: "ending".into(),
            root: modification.path().into(),
        }],
    )?;
    let second = prepare(&modified, Diagnostics::new(true))?;
    let changed = Manifest::load(&second)?;
    assert_eq!(
        (changed.music.frames, changed.music.sample_rate),
        (1600, 16000)
    );
    assert_eq!(&*second.read("ui/credits/team.ktx2")?, picture(24)?);
    let game_over = resonance_game::game_over::Assets::from_prepared(second.clone())?;
    assert_eq!(game_over.art.caption, "A different ending");
    assert_eq!(game_over.art.choices, ["Resume", "Return"]);
    assert_eq!(&*first.read("ui/credits/team.ktx2")?, picture(16)?);
    assert_eq!(
        [original.pictures[0].width, original.pictures[0].height],
        [16, 16]
    );
    assert_eq!(
        [changed.pictures[0].width, changed.pictures[0].height],
        [24, 24]
    );
    assert!(
        changed
            .program
            .operations
            .iter()
            .any(|op| matches!(op, Operation::Text { text } if text=="Mod author"))
    );
    let font: BitmapFont = second.json("fonts/dialogue.json")?;
    let batches = layout(&changed, &font)?;
    assert_eq!(batches[2].positions[0][1], 240. - 56.);
    assert!(
        second
            .overrides()
            .iter()
            .any(|change| change.path == document::PATH || change.path.ends_with("credits.wav"))
    );
    let first_pcm = audio(&original, &first)?.decoder().count();
    let second_pcm = audio(&changed, &second)?.decoder().count();
    assert!(
        second_pcm > first_pcm * 9,
        "replacement duration did not affect actual playback"
    );
    assert_eq!(
        Manifest::load(&prepare(&base, Diagnostics::new(true))?)?
            .music
            .frames,
        320
    );
    Ok(())
}

#[test]
fn screen_discovery_follows_edited_branches_and_retains_dynamic_choices() -> Result<()> {
    let root = fixture()?;
    let library = Library::open(root.path(), &[])?;
    let path = "fields/ending/scenario.ssb";
    write(root.path(), path, &script("push.s8 17")?)?;
    fs::remove_file(root.path().join(document::PATH))?;
    assert!(!Manifest::captured(&prepare(
        &library,
        Diagnostics::new(true)
    )?));
    // Even a currently unselected branch must prepare its valid screen choice.
    let branched = script("push.s8 1\ncalc 0\nbranch_false credits\nend\ncredits:\npush.s8 18")?;
    for bytes in [script("load.s32 0x800")?, branched] {
        write(root.path(), path, &bytes)?;
        let error = prepare(&library, Diagnostics::new(false))
            .err()
            .context("missing credits were ignored")?;
        let text = format!("{error:#}");
        assert!(
            text.contains(document::PATH) && text.contains(path),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn screen_capture_rejects_invalid_media_and_preserves_optional_background_policy() -> Result<()> {
    let root = fixture()?;
    let library = Library::open(root.path(), &[])?;
    let first = prepare(&library, Diagnostics::new(true))?;
    write(root.path(), "audio/music/credits.wav", b"invalid WAV")?;
    let error = prepare(&library, Diagnostics::new(true))
        .err()
        .context("invalid music was reused")?;
    assert!(format!("{error:#}").contains("credits.wav"));
    assert_eq!(Manifest::load(&first)?.music.frames, 320);
    write(root.path(), "audio/music/credits.wav", &wave(32000, 320)?)?;
    write(root.path(), "ui/credits/team.ktx2", b"broken KTX2")?;
    let error = prepare(&library, Diagnostics::new(true))
        .err()
        .context("invalid image was accepted")?;
    assert!(format!("{error:#}").contains("team.ktx2"));
    write(root.path(), "ui/credits/team.ktx2", &picture(16)?)?;
    fs::remove_file(root.path().join("ui/game-over/background.ktx2"))?;
    assert!(prepare(&library, Diagnostics::new(true)).is_err());
    let diagnostics = Diagnostics::new(false);
    let tolerant = prepare(&library, diagnostics.clone())?;
    assert_eq!(diagnostics.entries()[0].scope, "game-over background");
    assert!(resonance_game::game_over::Assets::from_prepared(tolerant).is_ok());
    let calls = std::cell::Cell::new(0);
    assert!(
        resonance_game::screen_resources::capture(
            &mut library.snapshot(),
            ["fields/ending/scenario.ssb"],
            &Diagnostics::new(false),
            &|| {
                calls.set(calls.get() + 1);
                calls.get() > 3
            }
        )
        .is_err()
    );
    Ok(())
}

#[test]
#[ignore = "requires installed credits and their migration backup; compares actual drawing batches"]
fn compact_credits_keep_original_geometry_timing_and_image_pixels() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let library = root.join("local/layout-migration/final-integration-20261025-supervised/library");
    let backup = root.join("local/structure-review-20261027/credits-structure/before");
    let definition: document::Definition =
        serde_json::from_slice(&fs::read(library.join(document::PATH))?)?;
    let current = definition.prepare(|path| Ok(fs::read(library.join(path))?.into()))?;
    let old: serde_json::Value = serde_json::from_slice(&fs::read(backup.join(document::PATH))?)?;
    let font: BitmapFont = serde_json::from_slice(&fs::read(library.join("fonts/dialogue.json"))?)?;
    let atlas = resonance_asset_writer::texture::ktx2(&fs::read(library.join(&font.texture))?)?;
    let [x, y, w, h] = font.glyphs[&' '].rect;
    for row in y..y + h {
        for col in x..x + w {
            assert_eq!(
                atlas[0].rgba[((row * atlas[0].width + col) * 4 + 3) as usize],
                0,
                "removed whitespace must have no ink"
            );
        }
    }
    let mut original = current.clone();
    original.program = serde_json::from_value(old["program"].clone())?;
    original
        .program
        .operations
        .retain(|op| !matches!(op, Operation::Text { text } if text.chars().all(|c| c==' ')));
    for op in &mut original.program.operations {
        if let Operation::Picture { index } = op {
            *index = match *index {
                0 => 0,
                4 => 1,
                _ => panic!("unexpected credits picture"),
            };
        }
    }
    assert!(
        layout(&original, &font)? == layout(&current, &font)?,
        "credit drawing batches differ"
    );
    assert_eq!(
        serde_json::to_value(&current.program.scroll)?,
        old["program"]["scroll"]
    );
    assert_eq!(
        serde_json::to_value(&current.program.style)?,
        old["program"]["style"]
    );
    assert_eq!(serde_json::to_value(&definition.music)?, old["music"]);
    assert_eq!(
        definition.final_hold_ticks,
        old["final_hold_ticks"].as_u64().unwrap() as u32
    );
    for (picture, index) in current.pictures.iter().zip([0, 4]) {
        // The two used images were moved intact; the migration records their
        // inode/size and metadata. The source pixel check lives in the importer.
        assert_eq!(
            [picture.width, picture.height],
            [
                old["pictures"][index]["width"].as_u64().unwrap() as u32,
                old["pictures"][index]["height"].as_u64().unwrap() as u32
            ]
        );
    }
    eprintln!(
        "All visible glyph/image batches, spacing, scrolling, recording and final hold match layout 80"
    );
    Ok(())
}

#[test]
#[ignore = "requires a complete current named library; prepares the original ending route without devices"]
fn original_ending_route_captures_credits_and_game_over_content() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to current named content")?;
    let library = Library::open(&root, &[])?;
    let mut snapshot = library.snapshot();
    let entry = snapshot.field(534)?;
    let ending: resonance_content::field::document::Definition =
        snapshot.json(&entry, 1024 * 1024)?;
    let diagnostics = Diagnostics::new(true);
    resonance_game::screen_resources::capture(
        &mut snapshot,
        [ending.script.as_str()],
        &diagnostics,
        &|| false,
    )?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let credits = Manifest::load(&files)?;
    assert_eq!(credits.pictures.len(), 2);
    assert_eq!(credits.program.scroll.height_pixels, 20325);
    assert_eq!(credits.final_hold_ticks, 180);
    assert!(credits.music.frames > 0);
    let font: BitmapFont = files.json("fonts/dialogue.json")?;
    let meshes = layout(&credits, &font)?;
    assert!(!meshes.is_empty());
    for mesh in &meshes {
        assert!(
            mesh.positions
                .iter()
                .flatten()
                .all(|component| component.is_finite())
        );
        assert!(
            mesh.uv
                .iter()
                .flatten()
                .all(|component| component.is_finite())
        );
    }
    let game_over = resonance_game::game_over::Assets::from_prepared(files.clone())?;
    assert_eq!(game_over.art.choices, ["Load data", "Quit game"]);
    assert!(
        files
            .keys()
            .all(|path| !path.starts_with("assets/") && !path.ends_with(".preload.json"))
    );
    Ok(())
}
