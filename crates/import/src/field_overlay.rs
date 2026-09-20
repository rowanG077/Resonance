//! Bind script texture banks once; location lettering is an optional controller.
use crate::{field::MapArchive, field_resources, texture, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    effect::{CaptionSprite, LocationCaption, OverlayArt, OverlayTexture},
    font::UiTexture,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_script::NativeCall;

pub(crate) fn cook(
    extracted: &Path,
    map: &MapArchive,
    physical: &crate::scene::binding::Map<'_>,
    output: &Path,
    texture_ids: &std::collections::BTreeSet<u32>,
) -> Result<(BTreeMap<i32, String>, Vec<String>)> {
    let banks = bind(extracted, map, physical, output, texture_ids)?;
    let mut overlays = BTreeMap::new();
    let mut files = Vec::new();
    for (resource, bank) in banks {
        files.extend(
            bank.textures
                .iter()
                .flat_map(|texture| texture.images.iter().map(|image| image.path.clone())),
        );
        let bytes = serde_json::to_vec(&bank)?;
        let path = format!("fields/overlays/{}.json", crate::digest(&bytes));
        write_atomic(&output.join(&path), &bytes)?;
        files.push(path.clone());
        overlays.insert(resource, path);
    }
    files.sort();
    files.dedup();
    Ok((overlays, files))
}

fn bind(
    extracted: &Path,
    map: &MapArchive,
    physical: &crate::scene::binding::Map<'_>,
    output: &Path,
    texture_ids: &std::collections::BTreeSet<u32>,
) -> Result<BTreeMap<i32, OverlayArt>> {
    let script = map.section(6)?;
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let mut resources = field_resources::binding::Resources::open(output, extracted, &catalogue)?;
    let mut textures = texture_ids
        .iter()
        .map(|&id| Ok((id, texture::bind(output, &resources.directory(id)?)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    // Map-local texture handles index the resource tail, independent of which
    // branch creates an overlay. Bind every TPL, including unused entries.
    for index in 16..map.sections.len() {
        if map
            .optional_section(index)
            .is_some_and(|bytes| bytes.starts_with(&0x0020af30u32.to_be_bytes()))
        {
            let directory = physical
                .section(index)
                .context("missing cooked overlay section")?;
            textures.insert(
                0xffee0000 | (index as u32 - 16),
                texture::bind(output, &directory)?,
            );
        }
    }
    let mut banks = textures
        .into_iter()
        .map(|(resource, textures)| {
            (
                resource as i32,
                OverlayArt {
                    textures: textures
                        .into_iter()
                        .map(|texture| OverlayTexture {
                            images: texture
                                .images
                                .into_iter()
                                .map(|path| UiTexture {
                                    path,
                                    width: texture.dimensions[0].into(),
                                    height: texture.dimensions[1].into(),
                                })
                                .collect(),
                            sampler: texture.sampler,
                        })
                        .collect(),
                    caption: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for args in field_resources::literal_arguments(script, NativeCall::CreateOverlay, 13)? {
        if args[0].context("dynamic overlay controller needs a cooking recipe")? == 999_989 {
            let resource = args[1].context("dynamic location caption resource")?;
            let bank = banks
                .get_mut(&resource)
                .context("missing location caption texture bank")?;
            let images = bank
                .textures
                .iter()
                .map(|texture| texture.images[0].clone())
                .collect::<Vec<_>>();
            bank.caption = Some(LocationCaption {
                frames: frames(&images)?,
            });
        }
    }
    for bank in banks.values() {
        bank.validate()?;
    }
    Ok(banks)
}

fn frames(textures: &[UiTexture]) -> Result<Vec<Vec<CaptionSprite>>> {
    ensure!(
        (4..=13).contains(&textures.len()),
        "invalid location caption image count"
    );
    let main_width = textures[2].width;
    let multi = main_width > 8;
    let total_width: u32 = textures[3..].iter().map(|t| t.width).sum();
    let width = total_width.max(main_width);
    ensure!(width <= 640, "location caption exceeds screen width");
    let mut result = vec![Vec::new()]; // Initialization callback draws nothing.
    let (mut progress, mut bar_frame, mut phase, mut alpha, mut shine) = (0, 0, 0, 0, 0);
    let mut phase_done = false;
    let mut started = vec![0_u16; textures.len() - 3];
    for _ in 1..512 {
        let mut frame = Vec::new();
        let mut emit = |texture, rect, uv: [u8; 4], alpha| {
            frame.push(CaptionSprite {
                texture,
                rect,
                uv: uv.map(|v| f32::from(v) / 256.),
                alpha,
            })
        };
        progress += 8;
        let complete = progress > width;
        progress = progress.min(width);
        let half = (progress / 2) as f32;
        emit(0, [-half, -4., half, 4.], [0, 0, 255, 255], 255);
        if complete {
            bar_frame += 1;
            if bar_frame > 8 {
                bar_frame = 0;
                phase += 1;
                alpha = 0;
                if phase >= 5 {
                    phase = 4;
                    phase_done = true;
                }
            }
            if phase <= 3 {
                alpha = (alpha + 31).min(255);
            }
            let offset = (width / 2) as f32 + 32.;
            for (x, u0, u1) in [(-offset, 0, 127), (offset, 128, 255)] {
                let rect = [x - 32., -32., x + 32., 32.];
                if phase <= 3 {
                    emit(
                        1,
                        rect,
                        [u0, (phase + 1) * 40, u1, (phase + 2) * 40],
                        alpha as u8,
                    );
                }
                emit(1, rect, [u0, phase * 40, u1, (phase + 1) * 40], 255);
            }
        }
        if phase_done {
            if multi {
                shine = (shine + 8).min(255);
                let half = (main_width / 2) as f32;
                emit(2, [-half, -52., half, -4.], [0, 0, 255, 255], shine as u8);
            }
            if (!multi || shine == 255) && started[0] == 0 {
                started[0] = 1;
            }
        }
        let mut x = -((total_width / 2) as f32);
        let y = if multi { 4. } else { -64. };
        for i in 0..started.len() {
            if started[i] == 0 {
                continue;
            }
            started[i] = (started[i] + 10).min(255);
            if started[i] >= 128 && i + 1 < started.len() && started[i + 1] == 0 {
                started[i + 1] = 1;
            }
            let right = x + textures[i + 3].width as f32;
            emit(
                i + 3,
                [x, y, right, y + 64.],
                [0, 0, 255, 255],
                started[i] as u8,
            );
            x = right;
        }
        result.push(frame);
        if started.iter().all(|alpha| *alpha == 255) {
            return Ok(result);
        }
    }
    anyhow::bail!("location caption animation did not settle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both original discs and cook-all; binds overlays without conversion or publication"]
    fn original_route_and_setup_overlays_use_complete_shared_texture_banks() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let output = local.join("all-assets");
        let mut counts = [0; 3];
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let files = extracted.join("files");
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            let sources =
                std::iter::once("MAP/_custom.bin".to_owned()).chain((330..=341).map(|id| {
                    crate::field::source_for_id(&extracted, id)
                        .unwrap()
                        .strip_prefix(&files)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned()
                }));
            for source in sources {
                let map = MapArchive::open(&files.join(&source))?;
                let physical =
                    crate::scene::binding::Map::open(&output, disc, &source, &map.source_sha256)?;
                let declared = field_resources::declarations(map.section(6)?)?;
                let resources =
                    crate::character::Sources::read(&files, &catalogue, &declared.resources)?;
                let banks = bind(&extracted, &map, &physical, &output, &resources.textures)?;
                for id in resources.textures {
                    assert!(
                        banks.contains_key(&(id as i32)),
                        "{source}: missing texture {id:#x}"
                    );
                }
                for bank in banks.values() {
                    bank.validate()?;
                    for texture in &bank.textures {
                        for image in &texture.images {
                            assert!(image.path.starts_with("assets/"));
                            assert!(output.join(&image.path).is_file());
                            counts[2] += 1;
                        }
                    }
                    counts[1] += usize::from(bank.caption.is_some());
                }
                if source == "MAP/_custom.bin" {
                    assert_eq!(
                        banks.keys().copied().collect::<Vec<_>>(),
                        [0x31, 0x32, 0x33]
                    );
                    assert_eq!(
                        banks[&0x33]
                            .textures
                            .iter()
                            .map(|texture| (texture.images[0].width, texture.images[0].height))
                            .collect::<Vec<_>>(),
                        [(80, 72), (48, 72), (80, 72), (24, 72)]
                    );
                }
                if let Some(tutorial) = banks.get(&0x26) {
                    assert_eq!(tutorial.textures.len(), 1);
                    assert_eq!(
                        (
                            tutorial.textures[0].images[0].width,
                            tutorial.textures[0].images[0].height
                        ),
                        (259, 82)
                    );
                }
                counts[0] += banks.len();
            }
        }
        ensure!(
            counts[0] >= 8 && counts[1] > 0,
            "incomplete overlay source coverage"
        );
        eprintln!(
            "bound {} overlay banks, {} captions, {} images across both discs",
            counts[0], counts[1], counts[2]
        );
        Ok(())
    }

    #[test]
    fn iselia_reveal_matches_the_paused_dolphin_controller() {
        // GQSEAF school grounds: remaining hold 129/190, phase 1, frame 2,
        // bar alpha 93; neither the main plate nor lettering has started.
        let textures: Vec<_> = [(144, 8), (144, 512), (400, 48), (192, 64)]
            .into_iter()
            .map(|(width, height)| UiTexture {
                path: String::new(),
                width,
                height,
            })
            .collect();
        let track = frames(&textures).unwrap();
        let frame = &track[61];
        assert_eq!(frame.len(), 5);
        assert_eq!(frame[0].rect, [-200., -4., 200., 4.]);
        for (sprite, (rect, uv, alpha)) in frame[1..].iter().zip([
            ([-264., -32., -200., 32.], [0., 80., 127., 120.], 93),
            ([-264., -32., -200., 32.], [0., 40., 127., 80.], 255),
            ([200., -32., 264., 32.], [128., 80., 255., 120.], 93),
            ([200., -32., 264., 32.], [128., 40., 255., 80.], 255),
        ]) {
            assert_eq!(sprite.texture, 1);
            assert_eq!(sprite.rect, rect);
            assert_eq!(sprite.uv, uv.map(|v| v / 256.));
            assert_eq!(sprite.alpha, alpha);
        }
    }
}
