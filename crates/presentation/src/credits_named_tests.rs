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
        br#"{"version":1,"game":"GQSEAF","revision":0}"#,
    )?;
    let glyphs: serde_json::Map<_, _> = (32u8..=126)
        .map(|c| {
            (
                char::from(c).to_string(),
                serde_json::json!({"rect":[0,0,1,1],"advance":1}),
            )
        })
        .collect();
    json(
        p,
        "fonts/dialogue.json",
        serde_json::json!({"version":1,"texture":"fonts/dialogue/atlas.png",
        "width":1,"height":1,"line_height":1,"glyphs":glyphs}),
    )?;
    write(p, "fonts/dialogue/atlas.png", b"opaque atlas bytes")?;
    let texture = serde_json::json!({"path":"ui/unused.png","width":1,"height":1});
    json(
        p,
        "ui/dialogue.json",
        serde_json::json!({"version":2,"font":"fonts/dialogue.json",
        "textures": vec![texture.clone();9], "cursor": texture,
        "selection":{"mode":0,"color":[255,255,255,255],"row_offsets":vec![0;9],"bob_amplitude":0,"bob_step":0}}),
    )?;
    json(
        p,
        game_over::document::PATH,
        serde_json::json!({"version":1,
        "background":{"path":"ui/game-over/background.png","width":640,"height":480},
        "font":"fonts/dialogue.json","caption":"Try again", "choices":["Load","Quit"]}),
    )?;
    write(p, "ui/game-over/background.png", b"original background")?;
    json(
        p,
        document::PATH,
        serde_json::json!({"version":1,
        "program":{"version":1,"canvas":[640,480],
            "style":{"glyph_size":[16,16],"color":[255,255,255,255],"line_height":20,"tab_width":32},
            "scroll":{"height_pixels":1000,"speed_divisor_ticks":100,"stop_margin_pixels":100},
            "operations":[{"op":"text","text":"Credits"},{"op":"picture","index":0}]},
        "pictures":[{"path":"ui/credits/pictures/team.png","width":16,"height":16}],
        "music":{"path":"audio/music/credits/music.wav"},"final_hold_ticks":180}),
    )?;
    write(p, "ui/credits/pictures/team.png", b"original picture")?;
    write(p, "audio/music/credits/music.wav", &wave(32000, 320)?)?;
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
    Files::from_snapshot(snapshot, &mut Default::default(), || false, diagnostics)
}
fn audio(manifest: &Manifest, files: &Files) -> Result<Audio> {
    Ok(Audio {
        clip: Arc::new(crate::field_audio::Clip::prepare(
            files.read(&manifest.music.asset.path)?,
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
        "audio/music/credits/music.wav",
        &wave(16000, 1600)?,
    )?;
    write(
        modification.path(),
        "ui/credits/pictures/team.png",
        b"edited picture",
    )?;
    let mut game_over: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join(game_over::document::PATH))?)?;
    game_over["caption"] = "A different ending".into();
    game_over["choices"] = serde_json::json!(["Resume", "Return"]);
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
    assert_eq!(
        &*second.read("ui/credits/pictures/team.png")?,
        b"edited picture"
    );
    let game_over = resonance_game::game_over::Assets::from_prepared(second.clone())?;
    assert_eq!(game_over.art.caption, "A different ending");
    assert_eq!(game_over.art.choices, ["Resume", "Return"]);
    assert_eq!(
        &*first.read("ui/credits/pictures/team.png")?,
        b"original picture"
    );
    assert!(
        second
            .overrides()
            .iter()
            .any(|change| change.path == document::PATH || change.path.ends_with("music.wav"))
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
    write(root.path(), "audio/music/credits/music.wav", b"invalid WAV")?;
    let error = prepare(&library, Diagnostics::new(true))
        .err()
        .context("invalid music was reused")?;
    assert!(format!("{error:#}").contains("music.wav"));
    assert_eq!(Manifest::load(&first)?.music.frames, 320);
    write(
        root.path(),
        "audio/music/credits/music.wav",
        &wave(32000, 320)?,
    )?;
    fs::remove_file(root.path().join("ui/game-over/background.png"))?;
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
    let files = Files::from_snapshot(snapshot, &mut Default::default(), || false, diagnostics)?;
    let credits = Manifest::load(&files)?;
    assert_eq!(credits.program.operations.len(), 2212);
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
