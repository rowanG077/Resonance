//! Bake location-name animation into screen-space sprites and ordinary textures.
use crate::{field::MapArchive, field_resources, tpl, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    effect::{CaptionSprite, LocationCaption},
    font::UiTexture,
};
use std::{collections::BTreeMap, path::Path};
use symphonia_script::NativeCall;

pub(crate) fn cook(
    map: &MapArchive,
    prefix: &str,
    output: &Path,
) -> Result<(BTreeMap<i32, String>, Vec<String>)> {
    let mut captions = BTreeMap::new();
    let mut files = Vec::new();
    for args in field_resources::literal_arguments(map.section(6)?, NativeCall::CreateOverlay, 13)?
    {
        // Generic overlays have their own renderer/resource recipes. Only this
        // script identity selects the location-lettering animation controller.
        if args[0].context("dynamic overlay identity needs a cooking recipe")? != 999_989 {
            continue;
        }
        let resource = args[1].context("dynamic location caption resource")?;
        if captions.contains_key(&resource) {
            continue;
        }
        ensure!(
            resource as u32 >> 16 == 0xFFEE,
            "overlay {resource} needs a cooking recipe"
        );
        let index = resource as u16;
        let decoded = tpl::decode(map.section(16 + usize::from(index))?)?;
        ensure!(
            (4..=13).contains(&decoded.len()),
            "invalid location caption archive"
        );
        let mut textures = Vec::new();
        for (i, (width, height, rgba)) in decoded.iter().enumerate() {
            let name = format!("{prefix}/caption-{index}-{i}");
            let path = format!("{name}.ktx2");
            crate::texture::cook(*width, *height, rgba, &output.join(&path))?;
            files.push(path.clone());
            textures.push(UiTexture {
                path,
                width: *width,
                height: *height,
            });
        }
        let frames = frames(&textures)?;
        let caption = LocationCaption { textures, frames };
        caption.validate()?;
        let path = format!("{prefix}/caption-{index}.json");
        write_atomic(&output.join(&path), &serde_json::to_vec(&caption)?)?;
        files.push(path.clone());
        captions.insert(resource, path);
    }
    Ok((captions, files))
}

fn frames(textures: &[UiTexture]) -> Result<Vec<Vec<CaptionSprite>>> {
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
