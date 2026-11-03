//! Normalize native projectile storage, then reuse the GC record/hit decoders.
use crate::{
    audio_effects, battle_normal::canonical, output, package::Package, pe, source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_projectile::{self, FireBall, ResidentSpell, Table};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let usual = source.require("btlusual.pac")?;
    output::json(
        &library.join(battle_projectile::PATH),
        &read(&image, &usual)?,
    )
}

pub(crate) fn read(image: &pe::Image<'_>, usual: &[u8]) -> Result<Table> {
    let package = Package::parse(usual)?;
    let source = package.member(7)?.context("missing resident projectiles")?;
    let length = source.len() / 400 * 400;
    ensure!(
        length > 0
            && source.len() == length.next_multiple_of(64)
            && source[length..].iter().all(|&b| b == 0),
        "invalid resident projectile padding"
    );
    let reset_scale = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    ensure!(reset_scale.is_finite(), "non-finite projectile reset scale");
    let records = source[..length]
        .as_chunks::<400>()
        .0
        .iter()
        .enumerate()
        .map(|(index, row)| {
            resonance_importer::decode_projectile(&canonical_record(row)?, reset_scale)
                .with_context(|| format!("resident projectile {index}"))
        })
        .collect::<Result<_>>()?;
    let parameters = package
        .member(9)?
        .context("missing resident spell parameters")?;
    let spell = |id: u16| -> Result<&[u8]> {
        let index = usize::from(id - 200);
        ensure!(
            index < pe::word(parameters, 0)? as usize,
            "missing spell {id}"
        );
        parameters
            .get(pe::word(parameters, 4 + index * 4)? as usize..)
            .context("spell outside parameters")
    };
    let hit = |bytes: &[u8], recoil| -> Result<resonance_content::battle_action::ActionHit> {
        let row = canonical(bytes, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
        let mut hit = resonance_importer::decode_spell_hit(&row, recoil)?;
        hit.sound = hit.sound.map(audio_effects::id).transpose()?;
        Ok(hit)
    };
    let mut resident_spells = BTreeMap::new();
    for (id, recoil) in [(200, 6), (204, 1), (208, 6), (212, 6), (216, 1), (220, 10)] {
        let source = spell(id)?;
        let rules = source
            .get(pe::word(source, 0)? as usize..pe::word(source, 4)? as usize)
            .context("missing resident hit rules")?;
        resident_spells.insert(
            id,
            ResidentSpell {
                duration: pe::half(source, 16)?,
                hit: hit(rules.get(..28).context("missing resident hit")?, recoil)?,
                followup_hit: if id == 220 {
                    Some(hit(
                        rules.get(28..56).context("missing resident followup")?,
                        7,
                    )?)
                } else {
                    None
                },
            },
        );
    }
    let support_durations = [239, 242, 245, 248, 254, 273, 274, 275]
        .into_iter()
        .map(|id| {
            let duration = pe::half(spell(id)?, 16)?;
            ensure!(duration > 0, "empty support spell {id}");
            Ok((id, duration))
        })
        .collect::<Result<_>>()?;
    let scalar =
        |address| -> Result<f32> { Ok(f32::from_bits(pe::word(image.slice(address, 4)?, 0)?)) };
    let forward = scalar(0x72f3b8)?;
    let small = scalar(0x882e7c)?;
    let wide = scalar(0x883068)?;
    Ok(Table {
        version: battle_projectile::VERSION,
        records,
        resident_spells,
        support_durations,
        fire_ball: Some(FireBall {
            velocities: [
                [small, wide, forward],
                [scalar(0x883070)?, scalar(0x88306c)?, forward],
                [wide, small, forward],
            ],
            height_offset: f64::from_le_bytes(image.slice(0x882b60, 8)?.try_into()?) as f32,
            center_height_scale: 2.,
        }),
    })
}

pub(crate) fn canonical_record(row: &[u8]) -> Result<Vec<u8>> {
    let fields = [(8, 4), (12, 2), (0x84, 2), (0x86, 2), (0x90, 2), (0x94, 4)]
        .into_iter()
        .chain(
            (0x24..0x58)
                .step_by(4)
                .chain((0x60..0x84).step_by(4))
                .map(|at| (at, 4)),
        )
        .collect::<Vec<_>>();
    canonical(row, 400, &fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; verifies Inspect Magic identity and native scan helper"]
    fn installed_inspect_magic_225_uses_the_resident_scan_helper() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        ensure!(
            pe::word(image.slice(0x8d84b0 + 225 * 96, 4)?, 0)? == 264,
            "Inspect Magic resident identity changed"
        );
        let descriptor = pe::word(image.slice(0x8e9b08 + 264 * 4, 4)?, 0)?;
        ensure!(
            pe::word(image.slice(descriptor, 4)?, 0)? == 0x417980,
            "Inspect Magic initializer changed"
        );
        for (at, bytes) in [
            (0x4179b6, &[0xe8, 0xf5, 0xa3, 4, 0][..]),
            (0x4179f4, &[0x68, 0x95, 0, 0, 0][..]),
            (
                0x4179db,
                &[0xc7, 0x44, 0x24, 0x38, 0x68, 0x68, 0x68, 0xff][..],
            ),
            (0x461e4d, &[0x80, 0xc9, 2][..]),
            (0x461e5f, &[0x3c, 4][..]),
        ] {
            ensure!(
                image.slice(at, bytes.len())? == bytes,
                "Inspect Magic source callback changed at {at:#x}"
            );
        }
        Ok(())
    }
}
