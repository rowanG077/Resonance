//! Recover the two fixed-origin summon callbacks from their original resources.
use super::*;
use resonance_content::battle::actions::summon::{SummonKind, SummonRecipe};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub kind: SummonKind,
    pub offsets: Vec<[f32; 3]>,
    pub presentation: StoredSpellPresentation,
    pub title: String,
}

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<SummonRecipe> {
    ensure!(
        arte.flags == 0x20840191,
        "unexpected summon technique flags"
    );
    let p = tables
        .summons
        .fixed
        .iter()
        .find(|p| p.kind.native() == arte.native_id as u16)
        .context("missing cooked fixed summon controller")?;
    let bundle = tables.bundle(p.kind.native())?;
    let duration = match p.kind {
        SummonKind::Light => 0,
        SummonKind::Birth => 180,
    };
    ensure!(
        bundle.phase(0)?.duration == duration
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0),
        "unexpected summon action phases"
    );
    let recipe = SummonRecipe {
        kind: p.kind,
        rule: bundle.phase_rule(0, 0)?,
        offsets: p.offsets.clone(),
        presentation: p.presentation,
        title: p.title.clone(),
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, initialize, cleanup, pattern, settings, title) = match native {
        290 => (SummonKind::Light, 0x8a580, 0x8a214, 0x866c, 0x8750, 0x8760),
        292 => (SummonKind::Birth, 0x8fdb0, 0x8fa3c, 0x941c, 0x94dc, 0x94ec),
        _ => bail!("unsupported native summon callback"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(kind.native() - 200) * 4)?;
    for (phase, handler) in [initialize, 0x37e48, cleanup].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected summon phase {phase}"
        );
    }
    let color: [u8; 4] = rel.at((4, pattern - 4))?[..4].try_into()?;
    let pattern = rel.at((4, pattern))?;
    let settings = rel.at((4, settings))?;
    let text = rel.at((4, title))?;
    Ok(Parameters {
        kind,
        offsets: decode_pattern(pattern, kind.count())?,
        presentation: StoredSpellPresentation {
            color,
            camera_distance: float(settings, 0)?,
            camera_elevation: float(settings, 4)?,
        },
        title: std::str::from_utf8(crate::read::c_string(text, 0)?)?.to_owned(),
    })
}

fn decode_pattern(bytes: &[u8], count: usize) -> Result<Vec<[f32; 3]>> {
    (0..count)
        .map(|i| {
            Ok([
                float(bytes, i * 12)?,
                float(bytes, i * 12 + 4)?,
                float(bytes, i * 12 + 8)?,
            ])
        })
        .collect()
}

#[test]
fn original_pattern_bytes_preserve_all_points_in_order() {
    use sha2::{Digest, Sha256};
    for (points, hash, count) in [
        (
            vec![
                -300., 0., 0., 700., 0., 700., -0., 0., -300., -700., 0., 700., 300., 0., 0.,
                -700., 0., -700., 0., 0., 300., 700., 0., -700., 0., 0., 0., -425., 0., 0., 550.,
                0., 550., -0., 0., -425., -550., 0., 550., 425., 0., 0., -550., 0., -550., 0., 0.,
                425., 550., 0., -550.,
            ],
            "182daa3ad35f30bdf5d42014d881c2b2edf9887274320ca7cd5b562ef275f3a6",
            17,
        ),
        (
            vec![
                0., 0., 0., 300., 0., 600., -300., 0., -600., 300., 0., 0., -600., 0., 300., 300.,
                0., -600., -300., 0., 0., 600., 0., 300., -600., 0., -300., 600., 0., -300., 0.,
                0., 300., 0., 0., -300., -300., 0., 600., 0., 0., 0.,
            ],
            "e252b8fe59a9c22fb6cc84ece5e0bd1325523eddfa08b1452ac160b829f3b394",
            14,
        ),
    ] {
        let bytes: Vec<u8> = points.into_iter().flat_map(f32::to_be_bytes).collect();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), hash);
        let pattern = decode_pattern(&bytes, count).unwrap();
        assert_eq!(pattern.len(), count);
        assert_eq!(
            pattern[0],
            if count == 17 {
                [-300., 0., 0.]
            } else {
                [0.; 3]
            }
        );
        assert_eq!(
            pattern[count - 1],
            if count == 17 {
                [550., 0., -550.]
            } else {
                [0.; 3]
            }
        );
        assert!(decode_pattern(&bytes[..bytes.len() - 1], count).is_err());
    }
}
