//! The original title bank directly into named, editable menu artwork.
use crate::{dol, field_core, menu_sprites, native_archive, text, world_cinematics};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    font::UiTexture,
    startup::{self, Artwork, Choice, Definition},
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) const IMAGES: [(usize, &str); 14] = [
    (0, "continue-alternate"),
    (2, "logo"),
    (3, "copyright"),
    (4, "new-game-highlighted"),
    (5, "new-game"),
    (6, "continue-highlighted"),
    (7, "continue"),
    (8, "customize-highlighted"),
    (9, "customize"),
    (10, "game-record-highlighted"),
    (11, "game-record"),
    (12, "sound-test-highlighted"),
    (13, "sound-test"),
    (15, "lloyd-in-a-field"),
];

type Sizes = BTreeMap<&'static str, [u32; 2]>;

fn texture(sizes: &Sizes, name: &str) -> UiTexture {
    let [width, height] = sizes[name];
    UiTexture {
        path: format!("ui/title/{name}.ktx2"),
        width,
        height,
    }
}

fn definition(sizes: &Sizes) -> Definition {
    let artwork = |image, selected: Option<&str>, choice, top| Artwork {
        image: texture(sizes, image),
        selected_image: selected.map(|name| texture(sizes, name)),
        choice,
        top,
    };
    Definition {
        day_field: 474,
        night_field: 531,
        artwork: vec![
            artwork("logo", None, None, 24.),
            artwork("copyright", None, None, 417.),
            artwork(
                "new-game",
                Some("new-game-highlighted"),
                Some(Choice::NewGame),
                295.,
            ),
            artwork(
                "continue",
                Some("continue-highlighted"),
                Some(Choice::Continue),
                324.,
            ),
            artwork(
                "customize",
                Some("customize-highlighted"),
                Some(Choice::Customize),
                353.,
            ),
        ],
        music: "audio/music/tales-of-symphonia.wav".into(),
        cues: "audio/menu.json".into(),
    }
}

fn field(extracted: &Path, exe: &[u8], id: usize) -> Result<Vec<u8>> {
    let rows = dol::slice(exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let row = &rows[id * 24..id * 24 + 24];
    field_core::native_archive(extracted, exe, row, id)?
        .with_context(|| format!("missing native title field {id}"))
}

fn title_field_art(extracted: &Path, exe: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let day = field(extracted, exe, 474)?;
    let night = field(extracted, exe, 531)?;
    let mut output = Vec::new();
    for (section, path) in [(16, "ui/title/camera-1.glb"), (23, "ui/title/camera-2.glb")] {
        let source = field_core::optional_section(&day, section)?
            .with_context(|| format!("missing title camera section {section}"))?;
        ensure!(
            field_core::optional_section(&night, section)? == Some(source),
            "day and night title cameras differ"
        );
        output.push((path.into(), world_cinematics::camera(source)?));
    }
    let section = 19;
    let source =
        field_core::optional_section(&day, section)?.context("missing title screen overlay")?;
    ensure!(
        field_core::optional_section(&night, section)? == Some(source),
        "day and night title overlays differ"
    );
    ensure!(dol::u32(source, 4)? == 1, "title overlay is not one image");
    let (width, height, rgba) = menu_sprites::decode_image(source, 0)?;
    output.extend(crate::field_overlays::image_files(
        crate::field_overlays::image("ui/title/title-screen.ktx2", [width, height]),
        resonance_asset_writer::gpu::encode_rgba8(width, height, &rgba)?,
    )?);
    Ok(output)
}

pub fn cook_title_screen(extracted: &Path, library: &Path) -> Result<()> {
    let disc = extracted.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let filename = text::string(&exe, 0x8017_d870)?;
    let tpl = native_archive::read_named(&disc.join("files"), &filename)?;
    ensure!(
        dol::u32(&tpl, 4)? == 18,
        "unexpected native title image count"
    );
    let mut output = title_field_art(extracted, &exe)?;
    let mut sizes = Sizes::new();
    for (index, name) in IMAGES {
        let (width, height, rgba) = menu_sprites::decode_image(&tpl, index)?;
        sizes.insert(name, [width, height]);
        output.push((
            format!("ui/title/{name}.ktx2"),
            resonance_asset_writer::gpu::encode_rgba8(width, height, &rgba)?,
        ));
    }
    let definition = definition(&sizes);
    definition.validate()?;
    for (path, bytes) in output {
        crate::output::write(&library.join(&path), &bytes)?;
    }
    crate::output::json(&library.join(startup::PATH), &definition)
}
