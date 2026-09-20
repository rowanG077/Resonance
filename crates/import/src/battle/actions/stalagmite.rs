//! Stalagmite's stored callback keeps all three independently authored contact rows.
use super::*;
use resonance_content::battle::actions::earth_field::{EarthField, EarthFieldRecipe};

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<EarthFieldRecipe> {
    ensure!(
        arte.native_id as u16 == 213 && arte.flags == 0x0044018b,
        "unexpected Stalagmite binding"
    );
    let bundle = tables.bundle(213)?;
    ensure!(
        bundle.phase(0)?.duration == 240
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 3,
        "unexpected Stalagmite phase/rule closure"
    );
    tables
        .stored
        .stalagmite
        .earth(EarthField::Stalagmite, bundle)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<stored_parameters::Ground> {
    let dispatch = rel.pointer(DATA, 0x1238 + 13 * 4)?;
    for (phase, callback) in [0x7239c, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, callback),
            "unexpected Stalagmite dispatch phase{phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x72280)),
        "missing Stalagmite callback"
    );
    for (offset, size, digest) in [
        (
            0x72280,
            0x11c,
            "ac5210c7ae3e46b3799aa044138a745ae00ca3be86bc9ab1743c575f8a30ed99",
        ),
        (
            0x7239c,
            0xe8,
            "6c561edbccfa4449cb5d151bf100884be9b0f26570403b199a571d6dc24aea56",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Stalagmite callback")?
            ) == digest,
            "unrecovered Stalagmite callback operation at {offset:#x}"
        );
    }
    stored_parameters::Ground::read(rel, 0x4730, 245, &[(44, 0, 0), (60, 1, 1), (90, 2, 2)])
}
