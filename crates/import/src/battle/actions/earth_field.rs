//! Ground Dasher and Grave retain the target point selected by the stored initializer.
use super::*;
use resonance_content::battle::actions::earth_field::{EarthField, EarthFieldRecipe};

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<EarthFieldRecipe> {
    let native = arte.native_id as u16;
    let (kind, rules, parameters) = match native {
        214 => (EarthField::GroundDasher, 1, &tables.stored.ground_dasher),
        215 => (EarthField::Grave, 2, &tables.stored.grave),
        _ => bail!("unsupported stored Earth native {native}"),
    };
    ensure!(arte.flags == 0x0044018b, "unexpected stored Earth binding");
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == rules,
        "unexpected Earth action phases or hit rules"
    );
    parameters.earth(kind, bundle)
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<stored_parameters::Ground> {
    let (initializer, callback, settings, lifetime, schedule): (_, _, _, _, &[(u16, u8, usize)]) =
        match native {
            214 => (0x87a70, 0x87a08, 0x8058, 190, &[(40, 1, 0)]),
            215 => (
                0x7e100,
                0x7e010,
                0x6098,
                215,
                &[(34, 0, 0), (78, 1, 1), (88, 1, 1), (98, 1, 1), (108, 1, 1)],
            ),
            _ => bail!("unsupported stored Earth native {native}"),
        };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    for (phase, handler) in [initializer, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Earth phase {phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, callback)),
        "missing Earth callback"
    );
    // Validate the whole recovered body, including branches and every argument
    // copy, before using the decoded schedule and retained target point.
    let bodies = if native == 214 {
        [
            (
                0x87a70,
                0xe8,
                "080a84822ca9c6da4064f47532325b1f845926840706d3222c07f854d8838391",
            ),
            (
                0x87a08,
                0x68,
                "ceeb5e781a2dac0bcf4ba91c0e6eb9e5d1360281a9c292765707212082184068",
            ),
        ]
    } else {
        [
            (
                0x7e100,
                0xe8,
                "11bb0f082dfa62d4b91e02acdb3da55fac44f38ddc63d3a90bfbbe7d3efe35b0",
            ),
            (
                0x7e010,
                0xf0,
                "7eb37ee2e5d4d7427b0f09119c68998e76e7b2cf5daedaf6595316779d386b88",
            ),
        ]
    };
    for (offset, size, digest) in bodies {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Earth callback")?
            ) == digest,
            "unrecovered Earth callback operation at {offset:#x}"
        );
    }
    let fallback = rel.at((4, 0x1c4c))?;
    ensure!(
        (0..3).all(|i| float(fallback, i * 4).ok() == Some(0.)),
        "unsupported stored Earth target fallback"
    );
    stored_parameters::Ground::read(rel, settings, lifetime, schedule)
}

#[cfg(test)]
#[path = "earth_field/tests.rs"]
mod tests;
