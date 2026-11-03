//! Ending artwork and staff roll use the same images, text blocks and WAV as GC.
use crate::{audio_streams, output, package::Package, pe, source::Source, text::Bank, ui_images};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    credits::{
        self, Scroll, Style,
        document::{Alignment, Block, Definition, Recording},
    },
    font::UiTexture,
    library::Mappings,
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const FIN: [&str; 4] = [
    "ui/ending/fin-f.ktx2",
    "ui/ending/fin-i.ktx2",
    "ui/ending/fin-n.ktx2",
    "ui/ending/fin-dot.ktx2",
];
const LINE_HEIGHT: i32 = 25;

/// Read only the ending's single ADX stream. Video packets remain on disk.
fn audio(input: &mut (impl Read + Seek)) -> Result<Vec<u8>> {
    let size = input.seek(SeekFrom::End(0))?;
    let (mut offset, mut count) = (0u64, 0usize);
    let (mut header_seen, mut ended) = (false, false);
    let mut data = Vec::new();
    while offset < size {
        ensure!(
            size - offset >= 32 && count < 2_000_000,
            "truncated or oversized ending USM"
        );
        input.seek(SeekFrom::Start(offset))?;
        let mut header = [0; 32];
        input.read_exact(&mut header)?;
        let length = u64::from(u32::from_be_bytes(header[4..8].try_into()?));
        let start = u64::from(header[9]);
        let padding = u64::from(u16::from_be_bytes(header[10..12].try_into()?));
        ensure!(
            header[8] == 0
                && header[13] == 0
                && header[14] <= 1
                && header[24..] == [0; 8]
                && start >= 24
                && start + padding <= length
                && offset + 8 + length <= size,
            "invalid ending USM chunk"
        );
        let bytes = usize::try_from(length - start - padding)?;
        ensure!(bytes <= 16 * 1024 * 1024, "ending USM chunk exceeds limit");
        ensure!(
            matches!(&header[..4], b"CRID" | b"@SFA" | b"@SFV"),
            "unknown ending USM stream"
        );
        if offset == 0 {
            ensure!(
                &header[..4] == b"CRID" && header[15] == 1,
                "ending USM has no directory"
            );
        }
        if &header[..4] == b"@SFA" {
            ensure!(
                header[12] == 0 && !ended,
                "ending USM has extra audio streams or data after EOF"
            );
            input.seek(SeekFrom::Start(offset + 8 + start))?;
            match header[15] {
                0 => {
                    ensure!(
                        header_seen && data.len() + bytes <= 64 * 1024 * 1024,
                        "invalid ending audio payload"
                    );
                    let at = data.len();
                    data.resize(at + bytes, 0);
                    input.read_exact(&mut data[at..])?;
                }
                1 => {
                    ensure!(!header_seen, "duplicate ending audio header");
                    header_seen = true;
                }
                2 => {
                    ensure!(header_seen && bytes <= 128, "invalid ending audio marker");
                    let mut marker = vec![0; bytes];
                    input.read_exact(&mut marker)?;
                    ended = marker.starts_with(b"#CONTENTS END");
                }
                other => anyhow::bail!("unsupported ending audio payload {other}"),
            }
        }
        offset += 8 + length;
        count += 1;
    }
    ensure!(
        offset == size && ended && !data.is_empty(),
        "incomplete ending ADX stream"
    );
    Ok(data)
}

fn gap(blocks: &mut Vec<Block>, height: &mut i32, pixels: i32) -> Result<()> {
    *height = height
        .checked_add(pixels)
        .context("credits height overflow")?;
    if pixels != 0 {
        if let Some(Block::Gap { gap }) = blocks.last_mut() {
            *gap = gap.checked_add(pixels).context("credits gap overflow")?;
        } else {
            blocks.push(Block::Gap { gap: pixels });
        }
    }
    Ok(())
}

/// A one-pixel tab retains native inline pixel gaps using the ordinary shared
/// text block format. Literal source tabs retain their 96-pixel advance.
fn compile(text: &str, pictures: &[UiTexture]) -> Result<(Vec<Block>, i32)> {
    let text = text.replace("\r\n", "\n");
    let (mut blocks, mut height) = (Vec::new(), 0i32);
    for raw in text.split_inclusive('\n') {
        let terminated = raw.ends_with('\n');
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let (mut literal, mut centered, mut picture, mut spacing) =
            (String::new(), false, None, None);
        let mut chars = line.chars().peekable();
        while let Some(character) = chars.next() {
            match character {
                '@' => match chars.next().context("truncated credits control")? {
                    'c' => {
                        ensure!(
                            literal.is_empty() && !centered,
                            "misplaced credits alignment"
                        );
                        centered = true;
                    }
                    's' => {
                        let digits = std::iter::from_fn(|| chars.next_if(char::is_ascii_digit))
                            .collect::<String>();
                        let pixels: usize =
                            digits.parse().context("invalid credits horizontal gap")?;
                        ensure!(pixels <= 4096, "credits horizontal gap exceeds limit");
                        literal.extend(std::iter::repeat_n('\t', pixels));
                    }
                    'f' => {
                        ensure!(
                            literal.is_empty() && chars.peek().is_none(),
                            "misplaced credits page gap"
                        );
                        spacing = Some(480);
                    }
                    '\\' => {
                        ensure!(literal.is_empty(), "inline credits vertical gap");
                        spacing = Some(
                            chars
                                .by_ref()
                                .collect::<String>()
                                .trim()
                                .parse()
                                .context("invalid credits vertical gap")?,
                        );
                    }
                    'p' => {
                        ensure!(literal.chars().all(|c| c == '\t'), "inline credits picture");
                        picture = Some(
                            chars
                                .by_ref()
                                .collect::<String>()
                                .parse::<usize>()
                                .context("invalid credits picture index")?,
                        );
                    }
                    other => anyhow::bail!("unsupported credits control {other:?}"),
                },
                '\t' => literal.extend(std::iter::repeat_n('\t', 96)),
                c => {
                    ensure!(!c.is_control(), "invalid credits character");
                    literal.push(c);
                }
            }
        }
        if let Some(pixels) = spacing {
            gap(&mut blocks, &mut height, pixels)?;
        } else if let Some(index) = picture {
            let image = pictures
                .get(index)
                .context("credits picture exceeds bank")?;
            let offset_x = if centered {
                (640. - image.width as f32) / 2.
            } else {
                literal.len() as f32
            };
            blocks.push(Block::Image {
                image: image.path.clone(),
                offset_x,
            });
            gap(&mut blocks, &mut height, image.height.try_into()?)?;
        } else if literal.is_empty() {
            if terminated {
                gap(&mut blocks, &mut height, LINE_HEIGHT)?;
            }
        } else {
            blocks.push(Block::Text {
                lines: vec![literal],
                align: if centered {
                    Alignment::Center
                } else {
                    Alignment::Left
                },
                line_spacing: None,
            });
            height = height
                .checked_add(LINE_HEIGHT)
                .context("credits height overflow")?;
            if !terminated {
                gap(&mut blocks, &mut height, -LINE_HEIGHT)?;
            }
        }
    }
    Ok((blocks, height))
}

fn legacy_music(source: &Source, library: &Path, image: &pe::Image<'_>) -> Result<bool> {
    let row = image
        .slice(0x8ad188, 114 * 8)?
        .chunks_exact(8)
        .find(|row| pe::half(row, 0).ok() == Some(42156))
        .context("missing legacy ending music declaration")?;
    let name = image.text(pe::word(row, 4)?)?;
    ensure!(
        pe::half(row, 2)? == 4 && name.eq_ignore_ascii_case("tos_ending.adx"),
        "ending music declaration changed"
    );
    let raw = source.root.join(audio_streams::RAW).join(name);
    let bytes = if source.contains(name)? {
        Some(source.require(name)?)
    } else if raw.is_file() {
        Some(fs::read(raw)?)
    } else {
        None
    };
    let Some(bytes) = bytes else { return Ok(false) };
    audio_streams::wave(&bytes, &library.join("audio/music/tos-ending.wav"))?;
    let mapping_path = library.join(resonance_content::field_audio::document::MUSIC);
    let existing: Mappings = serde_json::from_slice(&fs::read(&mapping_path)?)?;
    let mut mappings = existing
        .iter()
        .map(|(id, name)| (id, name.to_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    ensure!(
        mappings
            .insert(42156, "tos-ending.wav".into())
            .is_none_or(|previous| previous == "tos-ending.wav"),
        "ending music already has a different owner"
    );
    output::json(&mapping_path, &Mappings::new(mappings)?)?;
    Ok(true)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    for (address, expected) in [
        (0x552e00, &[0x68, 0x7c, 0x41, 0x87, 0][..]),
        (0x552c32, &[0x8b, 0x49, 4]),
        (0x553543, &[0x68, 0x10, 0x41, 0x87, 0]),
        (0x552e61, &[0xc7, 0x05, 0xc0, 0x85, 0xb1, 0]),
        (0x552e3e, &[0x68, 0x88, 0x41, 0x87, 0]),
    ] {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "ending consumer changed at {address:#x}"
        );
    }
    for (id, path) in [
        (49, "ui/ending/party-with-kratos.ktx2"),
        (50, "ui/ending/party-with-zelos.ktx2"),
    ] {
        let name = image.text(pe::word(image.slice(0x8adb10 + id * 4, 4)?, 0)?)?;
        ui_images::cook(source, library, name, path, true)?;
    }
    let bytes = source.require(image.text(pe::word(image.slice(0x8adb10 + 51 * 4, 4)?, 0)?)?)?;
    let fin = Package::parse(&bytes)?;
    ensure!(fin.len() == FIN.len(), "ending FIN image bank changed");
    for (index, path) in FIN.iter().enumerate() {
        ui_images::cook(
            source,
            library,
            fin.reference(index)?.context("missing FIN image")?,
            path,
            true,
        )?;
    }
    output::json(&library.join("ui/ending/fin.json"), &FIN)?;
    let bytes = source.require(image.text(0x87417c)?)?;
    let text = Bank::parse(&bytes)?.plain(1, encoding_rs::SHIFT_JIS)?;
    let bytes = source.require(image.text(0x874110)?)?;
    let artwork = Package::parse(&bytes)?;
    let mut pictures = Vec::new();
    for index in 0..artwork.len() {
        pictures.push(ui_images::cook(
            source,
            library,
            artwork.reference(index)?.context("missing credits image")?,
            &format!("ui/credits/picture-{index:02}.ktx2"),
            true,
        )?);
    }
    let (blocks, height_pixels) = compile(&text, &pictures)?;
    let movie = image
        .text(0x874188)?
        .strip_prefix("/MOVIE/")
        .context("ending outside movie directory")?;
    crate::source::validate_name(movie)?;
    ensure!(movie == "TOS_PS2_ED_720P.USM", "unrecognized ending movie");
    let mut input = fs::File::open(
        source
            .root
            .join("P12E/Data/TOS_Server/Rawdata/WIN/MOVIE")
            .join(movie),
    )?;
    let audio = audio(&mut input)?;
    let wave = library.join("audio/music/credits.wav");
    ensure!(
        audio_streams::wave(&audio, &wave)?.is_none(),
        "ending soundtrack unexpectedly loops"
    );
    let final_hold_ticks = pe::word(image.slice(0x552e67, 4)?, 0)?
        .checked_mul(2)
        .context("ending hold overflow")?;
    let definition = Definition {
        style: Style {
            glyph_size: [23, 25],
            color: [255; 4],
            line_height: LINE_HEIGHT as u16,
            tab_width: 1,
        },
        scroll: Scroll {
            height_pixels,
            speed_divisor_ticks: 21_600,
            stop_margin_pixels: 640,
        },
        blocks,
        music: Recording {
            path: "audio/music/credits.wav".into(),
            sample_rate: None,
        },
        final_hold_ticks,
    };
    output::json(&library.join(credits::document::PATH), &definition)?;
    // This declared legacy ADX is not in the installed sound bank. The USM's
    // recording above is owned by credits, not an invented alias for this ID.
    let legacy = legacy_music(source, library, &image)?;
    eprintln!(
        "  Ending: two portraits, four FIN letters, {} credits pictures",
        pictures.len()
    );
    if !legacy {
        eprintln!(
            "  Legacy music 42156 (tos_ending.adx) has no installed recording; credits uses its authored USM soundtrack"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, path::PathBuf};
    #[test]
    fn staff_controls_keep_columns_pictures_page_gaps_and_text() -> Result<()> {
        let pictures = [UiTexture {
            path: "ui/logo.ktx2".into(),
            width: 100,
            height: 50,
        }];
        let (blocks, height) = compile(
            "@f\r\n@cFirst@s48Second\r\n@\\6\r\n@c@p0\r\nLast",
            &pictures,
        )?;
        assert_eq!(height, 561);
        assert!(
            matches!(&blocks[1], Block::Text { lines, align: Alignment::Center, .. } if lines[0] == format!("First{}Second", "\t".repeat(48)))
        );
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, Block::Image { offset_x, .. } if *offset_x == 270.))
        );
        assert!(compile("@x", &pictures).is_err());
        assert!(compile("@p1\n", &pictures).is_err());
        Ok(())
    }
    fn chunk(kind: &[u8; 4], mode: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 32];
        bytes[..4].copy_from_slice(kind);
        bytes[4..8].copy_from_slice(&(24u32 + payload.len() as u32).to_be_bytes());
        bytes[9] = 24;
        bytes[15] = mode;
        bytes.extend(payload);
        bytes
    }
    #[test]
    fn ending_audio_packets_keep_order_while_video_stays_out() -> Result<()> {
        let bytes = [
            chunk(b"CRID", 1, b"directory"),
            chunk(b"@SFA", 1, b"header"),
            chunk(b"@SFA", 0, b"first"),
            chunk(b"@SFV", 0, b"video"),
            chunk(b"@SFA", 0, b"second"),
            chunk(b"@SFA", 2, b"#CONTENTS END\0"),
        ]
        .concat();
        assert_eq!(audio(&mut Cursor::new(bytes.clone()))?, b"firstsecond");
        assert!(audio(&mut Cursor::new(&bytes[..bytes.len() - 1])).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; imports ending audio/artwork and captures shared credits"]
    fn installed_ending_cooks_and_captures_shared_credits() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root)?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        credits::document::capture(&mut library.snapshot(), &|| false)?;
        let definition: Definition =
            serde_json::from_slice(&fs::read(root.join(credits::document::PATH))?)?;
        let manifest = definition.prepare(|path| Ok(fs::read(root.join(path))?.into()))?;
        ensure!(
            manifest.pictures.len() == 2
                && manifest.music.channels == 2
                && manifest.music.source_sample_rate == 48000,
            "ending media changed"
        );
        ensure!(
            manifest.music.frames == 18_258_240 && manifest.final_hold_ticks == 360,
            "ending audio clock changed"
        );
        eprintln!(
            "Prepared credits with {} operations and {} audio frames",
            manifest.program.operations.len(),
            manifest.music.frames
        );
        Ok(())
    }
}
