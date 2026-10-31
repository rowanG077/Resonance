//! Original credits controls directly into editable lines, pictures and music.
use crate::{audio_speech, native_archive, text, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::credits::{
    Scroll, Style,
    document::{Alignment, Block, Definition, Recording},
};
use std::{collections::VecDeque, fs, path::Path};

const LINE_HEIGHT: i32 = 25;

fn decimal(mut bytes: &[u8]) -> i32 {
    while matches!(bytes.first(), Some(b' ' | b'\t'..=b'\r')) {
        bytes = &bytes[1..];
    }
    let negative = bytes.first() == Some(&b'-');
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        bytes = &bytes[1..];
    }
    let limit = i32::MAX as u32 + u32::from(negative);
    let value = bytes
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0u32, |value, byte| {
            value
                .saturating_mul(10)
                .saturating_add(u32::from(byte - b'0'))
                .min(limit)
        });
    if negative {
        value.wrapping_neg() as i32
    } else {
        value as i32
    }
}

fn spacing(bytes: &[u8], at: usize) -> Result<(i32, usize)> {
    let end = bytes[at..]
        .iter()
        .position(|&byte| byte == b'\n')
        .context("unterminated credits spacing")?
        + at;
    Ok((decimal(&bytes[at..end.min(at + 62)]), end + 1))
}

fn scroll_height(bytes: &[u8]) -> Result<i32> {
    let mut height = 0i32;
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let advance = match byte {
            b'@' if bytes.get(at + 1) == Some(&b'\\') => {
                let (pixels, next) = spacing(bytes, at + 2)?;
                at = next;
                pixels
            }
            b'\n' => {
                at += 1;
                LINE_HEIGHT
            }
            _ => {
                at += if matches!(byte, 0x81..=0x9f | 0xe0..=0xfb) {
                    2
                } else {
                    1
                };
                0
            }
        };
        height = height
            .checked_add(advance)
            .context("credits height overflow")?;
    }
    Ok(height)
}

#[derive(Default)]
struct Line {
    bytes: Vec<u8>,
    centered: bool,
    picture: Option<u8>,
}

fn gap(blocks: &mut Vec<Block>, pixels: i32) -> Result<()> {
    if pixels != 0 {
        if let Some(Block::Gap { gap }) = blocks.last_mut() {
            *gap = gap.checked_add(pixels).context("credits gap overflow")?;
        } else {
            blocks.push(Block::Gap { gap: pixels });
        }
    }
    Ok(())
}

fn finish(line: &mut Line, advance: i32, blocks: &mut Vec<Block>) -> Result<()> {
    let (decoded, _, invalid) = encoding_rs::SHIFT_JIS.decode(&line.bytes);
    ensure!(!invalid, "invalid credits Shift-JIS text");
    let nonempty = !decoded.chars().all(|c| c == ' ');
    if let Some(index) = line.picture.take() {
        ensure!(
            !nonempty && !line.centered,
            "positioned inline credits image"
        );
        let name = match index {
            0 => "production-ig",
            1 => "avex-trax",
            2 => "day-after-tomorrow",
            3 => "polygon-magic",
            4 => "namco",
            _ => anyhow::bail!("unidentified credits image {index}"),
        };
        blocks.push(Block::Image {
            image: format!("ui/credits/{name}.ktx2"),
            offset_x: 0.,
        });
    } else if nonempty {
        blocks.push(Block::Text {
            lines: vec![decoded.into_owned()],
            align: if line.centered {
                Alignment::Center
            } else {
                Alignment::Left
            },
            line_spacing: None,
        });
    }
    gap(blocks, advance - if nonempty { LINE_HEIGHT } else { 0 })?;
    *line = Line::default();
    Ok(())
}

fn group_lines(blocks: Vec<Block>) -> Vec<Block> {
    let mut input: VecDeque<_> = blocks.into();
    let mut output = Vec::new();
    while let Some(block) = input.pop_front() {
        let Block::Text {
            mut lines, align, ..
        } = block
        else {
            output.push(block);
            continue;
        };
        let mut spacing = None;
        loop {
            let (gap, index) = match input.front() {
                Some(Block::Gap { gap }) => (*gap, 1),
                _ => (0, 0),
            };
            if !(0..LINE_HEIGHT).contains(&gap) {
                break;
            }
            let Some(Block::Text {
                align: next_align, ..
            }) = input.get(index)
            else {
                break;
            };
            let Some(next_spacing) = LINE_HEIGHT
                .checked_add(gap)
                .and_then(|value| u16::try_from(value).ok())
            else {
                break;
            };
            if align != *next_align
                || next_spacing == 0
                || spacing.is_some_and(|value| value != next_spacing)
            {
                break;
            }
            if index == 1 {
                input.pop_front();
            }
            let Some(Block::Text { lines: next, .. }) = input.pop_front() else {
                unreachable!()
            };
            lines.extend(next);
            spacing = Some(next_spacing);
        }
        output.push(Block::Text {
            lines,
            align,
            line_spacing: spacing.filter(|value| *value != LINE_HEIGHT as u16),
        });
    }
    output
}

fn blocks(source: &[u8]) -> Result<(Vec<Block>, i32)> {
    let end = source
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(source.len());
    ensure!(
        source[end..].iter().all(|&byte| byte == 0),
        "data after credits terminator"
    );
    let source = &source[..end];
    let height = scroll_height(source)?;
    let mut blocks = Vec::new();
    let mut line = Line::default();
    let mut at = 0;
    while at < source.len() {
        let byte = source[at];
        if matches!(byte, 0x81..=0x9f | 0xe0..=0xfb) {
            line.bytes.extend_from_slice(
                source
                    .get(at..at + 2)
                    .context("truncated credits Shift-JIS character")?,
            );
            at += 2;
            continue;
        }
        at += 1;
        match byte {
            b'\n' => finish(&mut line, LINE_HEIGHT, &mut blocks)?,
            b'\t' => line.bytes.push(byte),
            b'@' => {
                let code = *source.get(at).context("truncated credits control")?;
                at += 1;
                match code {
                    b'c' | b'C' => {
                        ensure!(
                            line.bytes.is_empty() && line.picture.is_none(),
                            "credits alignment in the middle of a line"
                        );
                        line.centered = true;
                    }
                    b'\\' => {
                        let (pixels, next) = spacing(source, at)?;
                        at = next;
                        finish(&mut line, pixels, &mut blocks)?;
                    }
                    b'p' | b'P' | b'w' | b'W' => {
                        let argument = *source
                            .get(at)
                            .context("truncated credits control argument")?;
                        at += 1;
                        if matches!(code, b'p' | b'P') {
                            ensure!(
                                line.picture.is_none()
                                    && !line.centered
                                    && line.bytes.iter().all(|&byte| byte == b' '),
                                "positioned inline credits image"
                            );
                            line.bytes.clear();
                            line.picture = Some(decimal(&[argument]) as u8);
                        }
                    }
                    _ => {}
                }
            }
            b'\r' => line.bytes.push(b' '),
            _ => line.bytes.push(byte),
        }
    }
    if !line.bytes.is_empty() || line.picture.is_some() || line.centered {
        finish(&mut line, 0, &mut blocks)?;
    }
    Ok((group_lines(blocks), height))
}

pub fn cook_credits_screen(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let files = extracted.join("disc1/files");
    let source = native_archive::read_named(&files, &text::string(&exe, 0x801d_facc)?)?;
    let (blocks, height_pixels) = blocks(&source)?;
    let pictures = native_archive::read_named(&files, &text::string(&exe, 0x801d_fa80)?)?;
    let mut outputs = Vec::new();
    for (index, name) in [(0, "production-ig"), (4, "namco")] {
        let path = format!("ui/credits/{name}.ktx2");
        ensure!(
            blocks
                .iter()
                .any(|block| matches!(block, Block::Image { image, .. } if image == &path)),
            "missing credits picture {index}"
        );
        outputs.push((path, world_texture::tpl_texture(&pictures, index)?.0));
    }
    let definition = Definition {
        style: Style {
            glyph_size: [23, 25],
            color: [255; 4],
            line_height: 25,
            tab_width: 96,
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
        final_hold_ticks: 180,
    };
    let mut json = serde_json::to_vec_pretty(&serde_json::to_value(&definition)?)?;
    json.push(b'\n');
    outputs.push(("ui/credits/credits.json".into(), json));
    let wave = library.join("audio/music/credits.wav");
    let adx = native_archive::read_named(&files, &text::string(&exe, 0x801d_fad8)?)?;
    for (path, bytes) in outputs {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    audio_speech::write_adx_wave(&adx, &wave)?;
    Ok(())
}
