//! Decode shared battle portraits, palette-selected sprites and text metrics.
mod icons;
mod scan;
mod status;
mod unison;
use super::message_tables::{self, Messages, ResultText};
use crate::{cooked::Source, digest};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::ui::{BattleUi, ComboLayout, DefeatLayout, NoticeLayout, ResultLabel, UiSprite},
    font::{BitmapFont, Glyph, UiRegion},
};
use std::{collections::BTreeMap, fs, path::Path};

fn hud(output: &Path, disc: u8, sources: &super::all::Sources) -> Result<crate::texture::Texture> {
    let mut textures = Source::open(output, disc, &sources.usual)?.textures("battle/usual/4/3")?;
    ensure!(textures.len() == 1, "missing cooked battle HUD atlas");
    let atlas = textures.pop().unwrap();
    ensure!(
        atlas.dimensions == [512, 512] && atlas.images.len() == 96,
        "unexpected cooked battle HUD palettes"
    );
    Ok(atlas)
}

pub(crate) fn cook(extracted: &Path, output: &Path, enemies: &[u8]) -> Result<BattleUi> {
    let disc = crate::disc_number(extracted)?;
    let sources = super::all::Sources::cooked(output, disc)?;
    let source = fs::read(extracted.join("files").join(&sources.usual))?;
    let executable = fs::read(extracted.join("files/US_r_Top2Btl.rel"))?;
    let data = output.join("data");
    let tables = crate::embedded::read::<super::ui_tables::UiTables>(
        &data,
        "battle-ui-tables",
        "US_r_Top2Btl.rel",
    )?;
    let messages =
        crate::embedded::read::<Messages>(&data, message_tables::FAMILY, "US_r_Top2Btl.rel")?;
    ensure!(
        messages.steal_format == "%s",
        "unsupported battle steal format"
    );
    ensure!(
        tables.texture_bindings[..5] == [10, 0, 11, 12, 1],
        "unsupported battle texture bindings"
    );
    let atlas = hud(output, disc, &sources)?;
    let mut sprite = |palette, rect| atlas.region(palette, rect);
    // This HUD font uses the union of the two cooked punctuation maps.
    const CELL: [u32; 2] = [16, 24];
    let mut glyphs = BTreeMap::new();
    for ch in b'!'..=b'Z' {
        let (x, y) = match ch {
            b'!'..=b'/' => {
                let index = usize::from(ch - b'!');
                let Some([x, y]) = [
                    tables.plain_punctuation[index],
                    tables.overlay_punctuation[index],
                ]
                .into_iter()
                .find(|&point| point != [0, 0]) else {
                    continue;
                };
                (u32::try_from(x)?, u32::try_from(y)?)
            }
            b'0'..=b'9' => (u32::from(ch - b'0') * 16, 0),
            b'A'..=b'F' => (160 + u32::from(ch - b'A') * 16, 0),
            b'G'..=b'V' => (u32::from(ch - b'G') * 16, 24),
            b'W'..=b'Z' => (u32::from(ch - b'W') * 16, 48),
            _ => continue,
        };
        glyphs.insert(
            char::from(ch),
            Glyph {
                rect: [x, y, CELL[0], CELL[1] - 1],
                advance: 13,
            },
        );
    }
    let font_image = atlas.image(0)?;
    let font = BitmapFont {
        version: 1,
        texture: font_image.path,
        width: font_image.width,
        height: font_image.height,
        line_height: CELL[1],
        glyphs,
        source_sha256: digest(&source),
        executable_sha256: digest(&executable),
    };
    use UiSprite::*;
    let status = Some(status::cook(&mut sprite)?);
    let unison = Some(unison::cook(tables.unison_palettes, &mut sprite)?);
    let sprites = [
        (TargetArrow, 16, [0, 256, 48, 64]),
        (TargetPlayerOne, 17, [48, 256, 144, 64]),
        (TargetPlayerTwo, 18, [48, 256, 144, 64]),
        (TargetPlayerThree, 19, [48, 256, 144, 64]),
        (TargetPlayerFour, 20, [48, 256, 144, 64]),
        (StrategyUp, 0, [224, 48, 24, 16]),
        (StrategyRight, 0, [192, 48, 16, 24]),
        (TargetLeft, 0, [256, 0, 24, 24]),
        (TargetRight, 0, [280, 0, 24, 24]),
        (CastingGlow, 22, [128, 72, 32, 32]),
        (CommandFrameShadow, 86, [0, 160, 48, 48]),
        (CommandFrame, 2, [0, 160, 48, 48]),
        (CommandTechA, 3, [49, 161, 46, 46]),
        (CommandTechB, 4, [97, 161, 46, 46]),
        (CommandTechC, 5, [145, 161, 46, 46]),
        (CommandUnisonA, 75, [97, 321, 46, 46]),
        (CommandUnisonB, 76, [145, 321, 46, 46]),
        (CommandUnisonC, 77, [193, 321, 142, 46]),
        (CommandStrategyA, 6, [193, 161, 46, 46]),
        (CommandStrategyB, 7, [241, 161, 46, 46]),
        (CommandStrategyC, 8, [289, 161, 46, 46]),
        (CommandStrategyD, 9, [337, 161, 46, 46]),
        (CommandEquipA, 10, [385, 161, 46, 46]),
        (CommandEquipB, 11, [433, 161, 46, 46]),
        (CommandItemA, 12, [1, 209, 46, 46]),
        (CommandItemB, 13, [49, 209, 46, 46]),
        (CommandItemC, 14, [97, 209, 46, 46]),
        (CommandEscapeLeg, 15, [145, 209, 46, 46]),
        (CommandCursor, 21, [160, 72, 32, 32]),
        (CommandDisabled, 47, [192, 256, 48, 48]),
        (CommandLabelShadow, 86, [304, 0, 80, 40]),
        (CommandLabel, 58, [304, 0, 80, 40]),
        // Preserve the fill margin sampled on the final update beyond full charge.
        (EscapeFill, 31, [312, 208, 136, 80]),
        (EscapeFrame, 29, [304, 208, 144, 80]),
        (EscapeNeedle, 30, [448, 208, 16, 32]),
        (UnisonPanel, 27, [0, 368, 352, 144]),
        (TechniqueGlowMiddle, 81, [448, 464, 24, 32]),
        (TechniqueGlowEdge, 81, [408, 464, 40, 48]),
        (TechniqueGlowRight, 81, [480, 456, 32, 40]),
        (TechniqueCapLeft, 80, [456, 408, 56, 48]),
        (TechniqueMiddle, 80, [416, 376, 96, 32]),
        (TechniqueCapRight, 80, [416, 408, 40, 40]),
        (BannerIcon, 80, [384, 384, 32, 32]),
        (BannerTech, 82, [384, 464, 24, 24]),
        (BannerSpell, 83, [384, 440, 24, 24]),
        (BannerItem, 84, [384, 488, 24, 24]),
        (BannerSystem, 85, [384, 416, 24, 24]),
        (ResultNextButton, 25, [384, 64, 32, 32]),
        (ResultNextButtonPressed, 25, [384, 96, 32, 32]),
        (CookButton, 24, [448, 64, 32, 32]),
        (CookButtonPressed, 24, [448, 96, 32, 32]),
    ]
    .into_iter()
    .map(|(id, palette, rect)| Ok((id, sprite(palette, rect)?)))
    .collect::<Result<_>>()?;
    let icons = tables
        .party_icons
        .iter()
        .enumerate()
        .map(|(index, rect)| {
            let id = (index + 1) as u8;
            let [x, y, width, height] = rect.map(u32::try_from);
            Ok((id, sprite(usize::from(id + 31), [x?, y?, width?, height?])?))
        })
        .collect::<Result<_>>()?;
    let portraits = Source::open(output, disc, &sources.usual)?
        .textures("battle/usual/4/2")?
        .into_iter()
        .enumerate()
        .map(|(i, image)| Ok(((i + 1) as u8, image.image(0)?)))
        .collect::<Result<_>>()?;
    let enemy_icons = icons::bind(output, crate::disc_number(extracted)?, &sources, enemies)?;
    let notices = NoticeLayout {
        labels: messages.notices.clone(),
        colors: tables.notices.colors,
        panels: tables.notices.panels,
    };
    let result_labels = result_labels(&messages)?;
    for text in result_labels
        .values()
        .chain(notices.labels.values().flatten())
    {
        for ch in text.chars().filter(|&ch| ch != ' ') {
            ensure!(
                font.glyphs.contains_key(&ch),
                "battle label {text:?} needs uncooked glyph {ch:?}"
            );
        }
    }
    let defeat_ui =
        crate::embedded::read::<crate::all_assets::DefeatUi>(&data, "defeat-ui", "main.dol")?;
    let title_images =
        Source::open(output, disc, &defeat_ui.background_source)?.standalone_textures()?;
    let backdrop = title_images
        .get(defeat_ui.background_image)
        .context("missing defeat background")?;
    let defeat = DefeatLayout {
        banner: messages.hud.defeat_banner.clone(),
        background: backdrop.image(0)?,
        caption: defeat_ui.caption().to_owned(),
        choices: defeat_ui.choices().map(str::to_owned),
    };
    let scan = scan::cook(&tables.scan, &mut sprite)?;
    let art = BattleUi {
        font,
        portraits,
        icons,
        enemy_icons,
        sprites,
        scan,
        status,
        unison,
        defeat,
        steal: messages.steal.clone(),
        escape_banner: messages.hud.escape_banner.clone(),
        notices,
        gauge_bonus_colors: tables.gauge_bonus[..2].try_into()?,
        target: tables.target.layout(messages.hud.cancel_orders.clone()),
        combo: ComboLayout {
            anchors: tables.combo.anchors,
            colors: tables.combo.colors[..4].try_into()?,
            panels: tables.combo.panels,
            hits: messages.hud.combo_hits.clone(),
            damage: messages.hud.combo_damage.clone(),
        },
        party: tables.party.panel(),
        damage: tables.damage,
        result_labels,
        result_messages: messages.result_messages.clone(),
        results: tables.results.layout(),
    };
    art.validate()?;
    Ok(art)
}

fn result_labels(messages: &Messages) -> Result<BTreeMap<ResultLabel, String>> {
    use ResultLabel::*;
    [
        (Experience, ResultText::Experience),
        (Bonus, ResultText::Bonus),
        (MaxCombo, ResultText::MaxCombo),
        (Gald, ResultText::Gald),
        (Time, ResultText::Time),
        (Grade, ResultText::PositiveGrade),
        (ItemsFound, ResultText::ItemsFound),
        (Cook, ResultText::CookHeading),
        (ExSkillEffect, ResultText::ExSkillEffect),
        (Info, ResultText::Info),
    ]
    .into_iter()
    .map(|(label, key)| {
        let text = messages
            .results
            .get(&key)
            .context("missing battle result text")?;
        Ok((
            label,
            text.split('%')
                .next()
                .unwrap()
                .trim_end_matches([' ', '+'])
                .to_owned(),
        ))
    })
    .collect()
}

#[test]
#[cfg(unix)]
#[ignore = "requires both original discs and cook-all; shared image binding without codecs or devices"]
fn original_battle_ui_binds_complete_shared_artwork() -> Result<()> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let library = project.join("local/all-assets");
    let output = crate::temporary_path(&std::env::temp_dir().join("battle-ui-binding"));
    fs::create_dir(&output)?;
    let result = (|| -> Result<()> {
        std::os::unix::fs::symlink(library.join("assets"), output.join("assets"))?;
        fs::copy(library.join("sources.json"), output.join("sources.json"))?;
        let mut previous = None;
        for disc in [1, 2] {
            let extracted = project.join(format!("local/extracted/disc{disc}"));
            let module = extracted.join("files/US_r_Top2Btl.rel");
            let data = output.join("data");
            let publications = super::all::Sources::publish(&module, &data)?
                .context("battle source publication")?;
            let index_path = output.join("sources.json");
            let mut index: BTreeMap<String, Vec<String>> =
                serde_json::from_slice(&fs::read(&index_path)?)?;
            let paths = index
                .get_mut(&format!("disc{disc}/US_r_Top2Btl.rel"))
                .context("battle source index")?;
            paths.retain(|path| !path.contains("/embedded/battle-sources/"));
            paths.extend(publications.into_iter().map(|path| format!("data/{path}")));
            fs::write(index_path, serde_json::to_vec(&index)?)?;
            super::ui_tables::cook_all(&module, &data)?;
            message_tables::cook_all(&module, &data)?;
            crate::all_assets::cook_defeat_ui(
                &extracted,
                &fs::read(extracted.join("sys/main.dol"))?,
                &data,
            )?;
            let art = cook(&extracted, &output, &[0, 36, 250])?;
            assert_eq!([art.font.width, art.font.height], [512, 512]);
            assert_eq!(art.portraits.len(), 9);
            assert_eq!(art.icons.len(), 9);
            assert_eq!(art.sprites.len(), UiSprite::ALL.len());
            for path in art.assets() {
                assert!(path.starts_with("assets/"), "unshared UI image {path}");
                assert_eq!(
                    &fs::read(output.join(path))?[..12],
                    b"\xabKTX 20\xbb\r\n\x1a\n"
                );
            }
            let atlas = hud(&output, disc, &super::all::Sources::cooked(&output, disc)?)?;
            for region in art.regions() {
                assert!(atlas.images.contains(&region.texture.path));
                assert_eq!([region.texture.width, region.texture.height], [512, 512]);
                region.validate()?;
            }
            let current = serde_json::to_value(&art)?;
            if let Some(previous) = &previous {
                assert_eq!(previous, &current);
            }
            previous = Some(current);
        }
        if let Some(path) = std::env::var_os("RESONANCE_UI_FIXTURE_OUTPUT") {
            crate::write_atomic(Path::new(&path), &serde_json::to_vec(&previous.unwrap())?)?;
        }
        Ok(())
    })();
    fs::remove_dir_all(output)?;
    result
}

#[test]
#[ignore = "requires both original extracted discs; reads message records without media conversion"]
fn original_battle_ui_labels_match_shared_messages() -> Result<()> {
    use resonance_content::battle::ui::NoticeKind;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in [1, 2] {
        let path = extracted.join(format!("disc{disc}/files/US_r_Top2Btl.rel"));
        let rel = crate::rel::Rel::read(&path)?;
        let (_, layout) =
            super::embedded::Layout::identify(&path).context("missing battle layout")?;
        let messages = message_tables::read(&rel, &layout)?;
        let notices = &messages.notices;
        let text = |pointer| -> Result<String> {
            Ok(std::str::from_utf8(crate::read::c_string(rel.at(pointer)?, 0)?)?.to_owned())
        };
        assert_eq!(notices.len(), NoticeKind::ALL.len());
        for (index, kind) in NoticeKind::ALL.into_iter().enumerate() {
            let line = |column| -> Result<String> {
                text(match kind {
                    NoticeKind::Overlimit => (4, 0x1690 + column * 8),
                    NoticeKind::DefenseDown => (4, [0x6598, 0x65a0][column]),
                    NoticeKind::AccuracyDown => (4, [0x6600, 0x6604][column]),
                    NoticeKind::EvasionDown => (4, [0x6f60, 0x6f68][column]),
                    NoticeKind::StatusCancel => (4, [0x1494, 0x15c4][column]),
                    _ => rel.pointer(4, 0x14f4 + index * 8 + column * 4)?,
                })
            };
            assert_eq!(notices[&kind], [line(0)?, line(1)?], "{kind:?}");
        }
        let labels = result_labels(&messages)?;
        assert_eq!(labels.len(), ResultLabel::ALL.len());
        for (label, offset) in ResultLabel::ALL.into_iter().zip([
            0x2e60, 0x2e6c, 0x2e78, 0x2e88, 0x2e94, 0x2eac, 0x2ed4, 0x2ee4, 0x2eec, 0x2efc,
        ]) {
            let source = text((4, offset))?;
            assert_eq!(
                labels[&label],
                source
                    .split('%')
                    .next()
                    .unwrap()
                    .trim_end_matches([' ', '+']),
                "{label:?}"
            );
        }
    }
    Ok(())
}
