//! Authored geometry and motion for the captured field image at battle entry.
use super::embedded::{self, Layout};
use crate::{
    read::{f32 as float, u32 as word},
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::entrance::{EntranceRecipe, POINT_COUNT, TRIANGLE_COUNT};
use serde::Serialize;
use serde_json::json;
use std::path::Path;

const POINT_BYTES: usize = POINT_COUNT * 12;
const TRIANGLE_BYTES: usize = TRIANGLE_COUNT * 12;

#[derive(Clone, Copy, Serialize)]
pub(super) struct EntranceLayout {
    /// Adjacent XYZ points and triangle indices in section 5.
    pub geometry: usize,
    /// Remaining roots address floats in section 4.
    pub native_size: usize,
    pub rotation: usize,
    pub center: usize,
    pub origin: [usize; 2],
    pub angular: usize,
    pub zero: usize,
}

#[derive(Serialize)]
struct AuthoredEntrance {
    #[serde(flatten)]
    recipe: EntranceRecipe,
    /// Retain the original draw plane even though the modern overlay uses XY.
    draw_depth: f32,
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    let rel = Rel::read(file)?;
    embedded::write(
        file,
        output,
        "battle-entrance",
        &read(&rel, layout.entrance)?,
        json!({
            "geometry_section":5, "constant_section":4, "layout":layout.entrance,
            "point_count":POINT_COUNT, "triangle_count":TRIANGLE_COUNT,
        }),
    )
    .map(Some)
}

fn read(rel: &Rel, layout: EntranceLayout) -> Result<AuthoredEntrance> {
    let geometry = rel
        .at((5, layout.geometry))?
        .get(..POINT_BYTES + TRIANGLE_BYTES)
        .context("truncated battle entrance geometry")?;
    let (points, triangles) = geometry.split_at(POINT_BYTES);
    let constant = |offset| float(rel.at((4, offset))?, 0);
    let recipe = EntranceRecipe {
        points: (0..POINT_COUNT)
            .map(|i| {
                ensure!(
                    float(points, i * 12 + 8)? == 0.,
                    "nonplanar battle entrance point"
                );
                Ok([float(points, i * 12)?, float(points, i * 12 + 4)?])
            })
            .collect::<Result<_>>()?,
        triangles: (0..TRIANGLE_COUNT)
            .map(|i| {
                Ok([
                    word(triangles, i * 12)?.try_into()?,
                    word(triangles, i * 12 + 4)?.try_into()?,
                    word(triangles, i * 12 + 8)?.try_into()?,
                ])
            })
            .collect::<Result<_>>()?,
        native_size: [
            constant(layout.native_size)?,
            constant(layout.native_size + 4)?,
        ],
        center_weight: constant(layout.center)?,
        initial_expansion: constant(layout.center + 4)?,
        outward_speed: constant(layout.angular + 8)?,
        angular_speed: constant(layout.angular)?,
        angular_step: constant(layout.angular + 4)?,
        angular_choices: 20,
        radians_per_degree: constant(layout.rotation)?,
        z_rotation_scale: constant(layout.rotation + 4)?,
    };
    ensure!(
        [constant(layout.origin[0])?, constant(layout.origin[1])?]
            == recipe.native_size.map(|v| v * 0.5)
            && constant(layout.zero)? == 0.,
        "unsupported battle entrance origin"
    );
    recipe.validate()?;
    Ok(AuthoredEntrance {
        recipe,
        draw_depth: constant(layout.rotation + 8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    #[ignore = "requires both original extracted discs; no cooking or output files"]
    fn original_entrance_geometry_and_motion_in_all_modules() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            for module in [
                "US_r_Top2Btl.rel",
                "r_Top2Btl.rel",
                "US_Top2Btl.rel",
                "US_m_Top2Btl.rel",
                "Top2Btl.rel",
                "m_Top2Btl.rel",
                "Top2BtlD.rel",
            ] {
                let file = extracted.join(format!("disc{disc}/files/{module}"));
                let (_, layout) = Layout::identify(&file).context("missing entrance layout")?;
                let layout = layout.entrance;
                let rel = Rel::read(&file)?;
                // Every root must be referenced by the original native consumers.
                let mut roots = vec![(5, layout.geometry), (5, layout.geometry + POINT_BYTES)];
                roots.extend(
                    [
                        layout.native_size,
                        layout.native_size + 4,
                        layout.rotation,
                        layout.rotation + 4,
                        layout.rotation + 8,
                        layout.center,
                        layout.center + 4,
                        layout.origin[0],
                        layout.origin[1],
                        layout.angular,
                        layout.angular + 4,
                        layout.angular + 8,
                        layout.zero,
                    ]
                    .map(|at| (4, at)),
                );
                ensure!(
                    roots.iter().all(|root| rel.local_targets().contains(root)),
                    "unreferenced entrance root in {module}"
                );
                let authored = read(&rel, layout)?;
                let bytes = serde_json::to_vec(&authored)?;
                ensure!(
                    bytes == *first.get_or_insert_with(|| bytes.clone()),
                    "changed entrance geometry or motion in disc{disc}/{module}"
                );
                let recipe = authored.recipe;
                assert_eq!(serde_json::from_slice::<EntranceRecipe>(&bytes)?, recipe);
                assert_eq!(authored.draw_depth, -0.5);
                assert_eq!(recipe.native_size, [640., 480.]);
                assert_eq!(recipe.center_weight, 0.333);
                assert_eq!(recipe.initial_expansion, 0.025);
                assert_eq!(
                    (
                        recipe.angular_speed,
                        recipe.angular_step,
                        recipe.angular_choices
                    ),
                    (1., 0.15, 20)
                );
                assert_eq!(recipe.outward_speed, 2.5);
                assert_eq!(recipe.radians_per_degree.to_bits(), 0x3c8e_fa35);
                assert_eq!(recipe.z_rotation_scale, 1.5);
                // All authored points participate; triangles cover the whole image.
                let mut used = BTreeSet::new();
                let mut area = 0.;
                for triangle in &recipe.triangles {
                    used.extend(triangle.iter().copied());
                    let [a, b, c] = triangle.map(|i| recipe.points[usize::from(i)].map(f64::from));
                    area +=
                        ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5;
                }
                assert_eq!(used.len(), POINT_COUNT);
                assert!((area - 640. * 480.).abs() < 0.1);
            }
        }
        Ok(())
    }

    #[test]
    fn entrance_decoder_rejects_truncation_and_invalid_geometry_or_motion() -> Result<()> {
        let layout = EntranceLayout {
            geometry: 0,
            native_size: 0,
            rotation: 8,
            center: 20,
            origin: [28, 32],
            angular: 36,
            zero: 48,
        };
        let mut bytes = vec![0];
        for value in [
            640f32,
            480.,
            0.017453292,
            1.5,
            -0.5,
            0.333,
            0.025,
            320.,
            240.,
            1.,
            0.15,
            2.5,
            0.,
        ] {
            bytes.extend(value.to_be_bytes());
        }
        let geometry = bytes.len();
        for i in 0..POINT_COUNT {
            for value in [(i % 8) as f32, (i / 8) as f32, 0.] {
                bytes.extend(value.to_be_bytes());
            }
        }
        for _ in 0..TRIANGLE_COUNT {
            for index in [0u32, 1, 8] {
                bytes.extend(index.to_be_bytes());
            }
        }
        let mut sections = vec![(0, 0); 6];
        sections[4] = (1, geometry - 1);
        sections[5] = (geometry, POINT_BYTES + TRIANGLE_BYTES);
        let mut rel = Rel {
            bytes,
            sections,
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        read(&rel, layout)?;
        rel.sections[5].1 -= 1;
        assert!(read(&rel, layout).is_err());
        rel.sections[5].1 += 1;
        rel.bytes[geometry + POINT_BYTES..geometry + POINT_BYTES + 4]
            .copy_from_slice(&(POINT_COUNT as u32).to_be_bytes());
        assert!(read(&rel, layout).is_err());
        rel.bytes[geometry + POINT_BYTES..geometry + POINT_BYTES + 4].fill(0);
        rel.bytes[geometry + 8..geometry + 12].copy_from_slice(&1f32.to_be_bytes());
        assert!(read(&rel, layout).is_err());
        rel.bytes[geometry + 8..geometry + 12].fill(0);
        rel.bytes[1 + layout.rotation + 8..1 + layout.rotation + 12]
            .copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(read(&rel, layout).is_err(), "nonfinite source draw depth");
        Ok(())
    }
}
