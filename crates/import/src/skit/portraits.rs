use super::*;
use resonance_content::skit::{PortraitAsset, PortraitFrame, PortraitVariant};

pub(super) fn cook(
    extracted: &Path,
    output: &Path,
    executable: &[u8],
) -> Result<BTreeMap<u32, PortraitAsset>> {
    let archive = fs::read(extracted.join("files/skit.skt"))?;
    let word = |at| -> Result<usize> {
        Ok(u32::from_be_bytes(
            archive
                .get(at..at + 4)
                .context("portrait archive table truncated")?
                .try_into()?,
        ) as usize)
    };
    let count = word(0)?;
    ensure!(count <= 230, "unexpected portrait count {count}");
    let mut result = BTreeMap::new();
    fs::create_dir_all(output.join("intermediate/skits"))?;
    fs::create_dir_all(output.join("game/skits/portraits"))?;
    for index in 0..count {
        let offset = word(4 + index * 8)?;
        let size = word(8 + index * 8)?;
        if size == 0 {
            continue;
        }
        let data = archive
            .get(offset..offset + size)
            .context("portrait outside archive")?;
        let textures = crate::tpl::decode(data).with_context(|| format!("portrait {index}"))?;
        let &(width, height, _) = textures.first().context("empty portrait")?;
        let ptr = u32::from_be_bytes(
            dol::slice(executable, 0x8020f49c + index as u32 * 4, 4)?.try_into()?,
        );
        let mut tracks: [Vec<PortraitFrame>; 3] = Default::default();
        let mut patches: [BTreeMap<u16, [u32; 2]>; 3] = Default::default();
        let mut repeat = [false; 3];
        if ptr != 0 {
            let mut at = ptr + 4;
            for _ in 0..4 {
                let head = dol::slice(executable, at, 4)?;
                let tag = u16::from_be_bytes(head[..2].try_into()?);
                if tag == 0xfefe {
                    break;
                }
                let channel = match tag {
                    0x7000 => 0,
                    0x7003 => 1,
                    0x7006 => 2,
                    _ => anyhow::bail!("portrait {index}: unknown timeline tag {tag:x}"),
                };
                let n = u16::from_be_bytes(head[2..].try_into()?);
                ensure!(
                    n <= 64 && tracks[channel].is_empty(),
                    "invalid portrait {index} timeline"
                );
                at += 4;
                for _ in 0..=n {
                    let row = dol::slice(executable, at, 8)?;
                    let v: Vec<_> = row
                        .chunks_exact(2)
                        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
                        .collect();
                    at += 8;
                    match v[2] {
                        0xfd => {
                            repeat[channel] = true;
                            break;
                        }
                        0xfe => break,
                        ticks => {
                            ensure!(
                                ticks < 253 && usize::from(v[3]) < textures.len(),
                                "portrait {index}: invalid patch"
                            );
                            patches[channel]
                                .insert(v[3], [u32::from(v[0]) / 8 * 8, u32::from(v[1]) / 8 * 8]);
                            tracks[channel].push(PortraitFrame { ticks, image: v[3] });
                        }
                    }
                }
            }
        }
        let choices: [Vec<u16>; 3] = std::array::from_fn(|i| {
            if patches[i].is_empty() {
                vec![0]
            } else {
                patches[i].keys().copied().collect()
            }
        });
        let count = choices.iter().map(Vec::len).product::<usize>();
        ensure!(count <= 512, "too many portrait expressions");
        let columns = (count as f32).sqrt().ceil() as u32;
        let rows = (count as u32).div_ceil(columns);
        // Each source portrait is clamped independently. Duplicate its edge
        // texels so linear filtering cannot reach the next atlas expression.
        let cell_width = width + 2;
        let cell_height = height + 2;
        let mut atlas = image::RgbaImage::new(cell_width * columns, cell_height * rows);
        let base = image::RgbaImage::from_raw(width, height, textures[0].2.clone())
            .context("portrait pixels")?;
        let mut variants = Vec::new();
        for &eye in &choices[0] {
            for &mouth in &choices[1] {
                for &extra in &choices[2] {
                    let images = [eye, mouth, extra];
                    let mut pixels = base.clone();
                    for channel in 0..3 {
                        if let Some(&[x, y]) = patches[channel].get(&images[channel]) {
                            let (w, h, rgba) = &textures[usize::from(images[channel])];
                            ensure!(
                                x + w <= width && y + h <= height,
                                "portrait {index}: patch outside image"
                            );
                            let patch = image::RgbaImage::from_raw(*w, *h, rgba.clone())
                                .context("patch pixels")?;
                            image::imageops::replace(
                                &mut pixels,
                                &patch,
                                i64::from(x),
                                i64::from(y),
                            );
                        }
                    }
                    let n = variants.len() as u32;
                    let x = n % columns * cell_width;
                    let y = n / columns * cell_height;
                    copy_with_gutter(&mut atlas, &pixels, x, y);
                    variants.push(PortraitVariant {
                        images,
                        rect: [x + 1, y + 1, width, height],
                    });
                }
            }
        }
        let texture = format!("game/skits/portraits/{index:03}.ktx2");
        let hash_path = output.join(format!("intermediate/skits/{index:03}.sha256"));
        let hash = crate::texture::fingerprint(atlas.width(), atlas.height(), atlas.as_raw());
        if fs::read_to_string(&hash_path).ok().as_deref() != Some(&hash)
            || !output.join(&texture).is_file()
        {
            crate::texture::cook(
                atlas.width(),
                atlas.height(),
                atlas.as_raw(),
                &output.join(&texture),
            )?;
            write_atomic(&hash_path, hash.as_bytes())?;
        }
        let layout_sha256 = crate::digest(&serde_json::to_vec(&(&tracks, &patches, repeat))?);
        result.insert(
            0xd0000 + index as u32,
            PortraitAsset {
                layout_sha256,
                texture,
                size: [width, height],
                atlas_size: [atlas.width(), atlas.height()],
                tracks,
                repeat,
                variants,
            },
        );
    }
    println!("Cooked {} portrait expression atlases", result.len());
    Ok(result)
}

fn copy_with_gutter(atlas: &mut image::RgbaImage, pixels: &image::RgbaImage, x: u32, y: u32) {
    let (width, height) = pixels.dimensions();
    for py in 0..height + 2 {
        for px in 0..width + 2 {
            let pixel = pixels.get_pixel(
                px.saturating_sub(1).min(width - 1),
                py.saturating_sub(1).min(height - 1),
            );
            atlas.put_pixel(x + px, y + py, *pixel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage, imageops::sample_bilinear};

    #[test]
    fn atlas_filtering_matches_the_standalone_clamped_portrait() {
        let portrait = RgbaImage::from_fn(2, 2, |x, y| {
            Rgba([40 + x as u8 * 180, y as u8 * 120, 80, (1 - x) as u8 * 255])
        });
        let mut atlas = RgbaImage::from_pixel(8, 8, Rgba([255; 4]));
        copy_with_gutter(&mut atlas, &portrait, 2, 2);
        // Include samples between an outermost texel's center and image edge,
        // where an unpadded atlas would pick up its opaque neighbour.
        for u in [0., 0.125, 0.25, 0.5, 0.75, 0.875, 1.] {
            for v in [0., 0.125, 0.25, 0.5, 0.75, 0.875, 1.] {
                assert_eq!(
                    sample_bilinear(&atlas, (3. + u * 2.) / 8., (3. + v * 2.) / 8.),
                    sample_bilinear(&portrait, u, v),
                    "portrait UV ({u}, {v})"
                );
            }
        }
    }
}
