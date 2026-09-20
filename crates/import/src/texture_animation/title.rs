use anyhow::{Context, Result, ensure};
use resonance_content::TextureAnimation;
use serde::{Deserialize, Serialize};
use std::path::Path;
use symphonia_script::NativeCall;

const PATH: &str = "embedded/title-texture-animations.json";

/// ConfigureRendering supplies the texture target for each callback slot.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
enum Slot {
    FirstVertical = 0,
    Horizontal = 1,
    SecondVertical = 2,
}

type Tracks = [TextureAnimation<Slot>; 3];

pub(crate) fn cook(executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::write_atomic(&output.join(PATH), &serde_json::to_vec(&read(executable)?)?)?;
    Ok(vec![PATH.into()])
}

pub(crate) fn bind(output: &Path, disc: u8, script: &[u8]) -> Result<Vec<TextureAnimation>> {
    let tracks: Tracks =
        crate::cooked::Source::open(output, disc, "sys/main.dol")?.document(PATH)?;
    ensure!(
        tracks.each_ref().map(|track| track.texture)
            == [Slot::FirstVertical, Slot::Horizontal, Slot::SecondVertical],
        "invalid title texture animation slots"
    );
    let mut targets = [None; 3];
    for args in
        crate::field_resources::literal_arguments(script, NativeCall::ConfigureRendering, 2)?
    {
        let command = args[0].context("dynamic title texture slot")?;
        if command > 7 {
            continue;
        }
        let slot = (command & 7) as usize;
        if slot >= targets.len() {
            continue;
        }
        let texture = usize::try_from(args[1].context("dynamic title texture target")?)?;
        ensure!(
            targets[slot]
                .replace(texture)
                .is_none_or(|old| old == texture),
            "conflicting title texture targets for slot {slot}"
        );
    }
    tracks
        .into_iter()
        .map(|track| {
            track.validate()?;
            Ok(TextureAnimation {
                texture: targets[track.texture as usize].context("missing title texture target")?,
                delay_ticks: track.delay_ticks,
                loop_start: track.loop_start,
                offsets: track.offsets,
            })
        })
        .collect()
}

fn read(executable: &[u8]) -> Result<Tracks> {
    let value = |address| crate::read::f32(crate::dol::slice(executable, address, 4)?, 0);
    ensure!(
        value(0x8035B25C)? == 0. && value(0x8035B29C)? == 1.,
        "unexpected title UV counter arithmetic"
    );
    bake(
        value(0x8035B354)?,
        value(0x8035B358)?,
        value(0x8035B360)?,
        value(0x8035B364)?,
        value(0x8035B368)?,
    )
}

fn bake(steps: f32, step_size: f32, first: f32, speed: f32, second: f32) -> Result<Tracks> {
    let integer = |value: f32| -> Result<usize> {
        ensure!(
            value.is_finite() && (1.0..=1000.0).contains(&value) && value.fract() == 0.,
            "invalid UV animation step count"
        );
        Ok(value as usize)
    };
    let steps = integer(steps)?;
    ensure!(
        step_size.is_finite() && step_size > 0. && step_size <= 1.,
        "invalid UV animation step size"
    );
    ensure!(
        speed.is_finite() && (1.0 / 36000.0..=1.0).contains(&speed),
        "invalid UV animation scroll speed"
    );
    let stepped = |texture, interval| -> Result<_> {
        let interval = integer(interval)?;
        let period = steps.checked_mul(interval).context("UV period overflow")?;
        ensure!(period <= 36000, "UV animation period exceeds limit");
        Ok(TextureAnimation {
            texture,
            delay_ticks: 1,
            loop_start: 0,
            offsets: (0..period)
                .map(|tick| [0., (tick / interval) as f32 * step_size])
                .collect(),
        })
    };
    let mut offsets = vec![[0., 0.]];
    let mut position = 0f32;
    loop {
        // Upload the accumulated f32 before resetting its overshooting value.
        position += speed;
        offsets.push([position, 0.]);
        ensure!(offsets.len() <= 36000, "UV scroll period exceeds limit");
        if position > 1. {
            break;
        }
    }
    Ok([
        stepped(Slot::FirstVertical, first)?,
        TextureAnimation {
            texture: Slot::Horizontal,
            delay_ticks: 1,
            loop_start: 1,
            offsets,
        },
        stepped(Slot::SecondVertical, second)?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs};

    #[test]
    fn sampled_light_motion_preserves_wrap_and_saved_state_phase() {
        let tracks = bake(32., 1. / 32., 22., 1. / 120., 7.).unwrap();
        for track in &tracks {
            track.validate().unwrap();
        }
        assert_eq!(tracks[0].offset(22), [0., 0.]);
        assert_eq!(tracks[0].offset(23), [0., 1. / 32.]);
        assert_eq!(tracks[0].offset(705), [0., 0.]);
        assert_eq!(tracks[1].offset(0), [0., 0.]);
        assert_eq!(tracks[1].offset(121)[0], 0.9999994);
        assert!(tracks[1].offset(122)[0] > 1.);
        assert_eq!(tracks[1].offset(123), [1. / 120., 0.]);
        // Independent Dolphin checkpoint, title idle counter 968.
        assert_eq!(tracks[0].offset(968), [0., 11. / 32.]);
        assert_eq!(tracks[1].offset(968), [0.9999994, 0.]);
        assert_eq!(tracks[2].offset(968), [0., 10. / 32.]);
        assert!(bake(0., 1., 22., 0.1, 7.).is_err());
        assert!(bake(32., 1., 22., f32::NAN, 7.).is_err());
        assert!(bake(32., 1., 22.5, 0.1, 7.).is_err());
    }

    fn script(bindings: &[(i32, i32)]) -> Result<Vec<u8>> {
        let mut text = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n".to_owned();
        for (slot, texture) in bindings {
            text.push_str(&format!(
                "push.s32 {slot}\ncalc 0\narg\npush.s32 {texture}\ncalc 0\narg\nproc 0x46\n"
            ));
        }
        text.push_str("end\n");
        Ok(symphonia_script::scenario::assemble(&text)?)
    }

    #[test]
    fn binding_uses_published_variants_and_authored_targets_without_an_executable() -> Result<()> {
        let output = crate::temporary_path(&std::env::temp_dir().join("title-uv"));
        let result = (|| -> Result<()> {
            let mut sources = BTreeMap::new();
            for (disc, speed) in [(1, 0.25), (2, 0.5)] {
                let prefix = format!("data/variants/{disc}");
                let tracks = bake(4., 0.25, 2., speed, 3.)?;
                crate::write_atomic(
                    &output.join(&prefix).join(PATH),
                    &serde_json::to_vec(&tracks)?,
                )?;
                sources.insert(
                    format!("disc{disc}/sys/main.dol"),
                    vec![format!("{prefix}/{PATH}")],
                );
            }
            crate::write_atomic(&output.join("sources.json"), &serde_json::to_vec(&sources)?)?;
            let authored = script(&[(0, 7), (1, 9), (2, 4), (128, 1)])?;
            for (disc, speed) in [(1, 0.25), (2, 0.5)] {
                let tracks = bind(&output, disc, &authored)?;
                assert_eq!(
                    tracks.iter().map(|track| track.texture).collect::<Vec<_>>(),
                    [7, 9, 4]
                );
                assert_eq!(tracks[1].offset(2), [speed, 0.]);
            }
            assert!(bind(&output, 1, &script(&[(0, 7), (1, 9)])?).is_err());
            assert!(bind(&output, 1, &script(&[(0, 7), (1, 9), (2, 4), (0, 8)])?).is_err());
            assert!(!output.join("sys/main.dol").exists());
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }

    #[test]
    #[ignore = "requires both original discs, cook-all, and the prepared title; no devices"]
    fn original_title_uv_publications_match_native_values_and_prepared_tracks() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let library = local.join("all-assets");
        let prepared: resonance_content::TitleAssets =
            serde_json::from_slice(&fs::read(local.join("cooked/title.json"))?)?;
        let expected = &prepared
            .scene
            .as_ref()
            .context("missing prepared title scene")?
            .parts
            .iter()
            .find(|part| part.resource == 2)
            .context("missing prepared lighting")?
            .texture_animations;
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let native = read(&executable)?;
            let expected_native = bake(32., 1. / 32., 22., 1. / 120., 7.)?;
            assert_eq!(
                serde_json::to_value(&native)?,
                serde_json::to_value(&expected_native)?
            );
            let published: Tracks =
                crate::cooked::Source::open(&library, disc, "sys/main.dol")?.document(PATH)?;
            assert_eq!(
                serde_json::to_value(&published)?,
                serde_json::to_value(&native)?
            );
            let source = crate::scene::title_source(&extracted, &executable)?;
            let hash = crate::media::hash_file(&extracted.join("files").join(&source))?;
            let map = crate::scene::binding::Map::open(&library, disc, &source, &hash)?;
            let script = fs::read(library.join(format!(
                "{}/script.ssb",
                map.section(6).context("missing script")?
            )))?;
            let tracks = bind(&library, disc, &script)?;
            assert_eq!(
                tracks.iter().map(|track| track.texture).collect::<Vec<_>>(),
                [2, 0, 1]
            );
            assert_eq!(
                serde_json::to_value(&tracks)?,
                serde_json::to_value(expected)?
            );
        }
        Ok(())
    }
}
