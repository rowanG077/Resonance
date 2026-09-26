//! Actor-hit geometry from original REL instructions 3BFD8..3C1D4.
//! The current C reconstruction of fn_1_3BDF8 does not recover these branches.
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitShape {
    Box,
    Cylinder,
    GroundCircle,
    Ring { width: f32 },
    Sphere,
}

/// One evaluated hurt point. Center is a world position; radius is the unscaled
/// model radius. Original packed bone flags/radius decoding belongs in loading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HurtPoint {
    pub center: [f32; 3],
    pub radius: f32,
}

/// Combat owns the evaluated body pose. Empty points mean no prepared hurt pose;
/// an animation sampler must update these same points when model playback lands.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub scale: f32,
    pub tint: [u8; 4],
    pub jitter: crate::BodyJitter,
    /// Original profile point in model space, separate from hurt bones.
    pub center_offset: [f32; 3],
    /// World point sampled before the actor callback; held during transitions.
    pub center: [f32; 3],
    /// 31C88 projects this sampled root at ground height into actor1A2C.
    pub audio_position: [f32; 3],
    /// 1B1DC bounds midpoint used for the target marker and target navigation.
    pub target_center: [f32; 3],
    pub points: Vec<HurtPoint>,
    /// Body-volume points (source bone flag 0x40) used for approach distance.
    /// Hurt contacts use the independent 0x20 set above.
    pub approach_points: Vec<HurtPoint>,
    /// Evaluated world positions for prepared body/weapon attachments, in binding order.
    pub anchors: Vec<[f32; 3]>,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            scale: 1.,
            tint: [64, 64, 64, 255],
            jitter: Default::default(),
            center_offset: [0.; 3],
            center: [0.; 3],
            audio_position: [0.; 3],
            target_center: [0.; 3],
            points: Vec::new(),
            approach_points: Vec::new(),
            anchors: Vec::new(),
        }
    }
}

impl Body {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.scale.is_finite()
                && self.scale > 0.
                && self.center_offset.iter().all(|v| v.is_finite())
                && self.target_center.iter().all(|v| v.is_finite())
                && self.points.len() <= 256
                && self.approach_points.len() <= 256
                && self.anchors.len() <= 256
                && self.anchors.iter().flatten().all(|v| v.is_finite())
                && self
                    .points
                    .iter()
                    .chain(&self.approach_points)
                    .all(|p| p.radius.is_finite()
                        && p.radius >= 0.
                        && p.center.iter().all(|v| v.is_finite())),
            "invalid battle hurt pose"
        );
        Ok(())
    }
}

impl crate::Actor {
    /// 1B3F0 / 31C88: scale, rotate, then add the current root position.
    pub(crate) fn sample_center(&mut self) -> Result<()> {
        self.body.audio_position = [self.position[0], 0., self.position[2]];
        let offset = rotate(
            self.body.center_offset.map(|v| v * self.body.scale),
            self.heading,
        );
        self.body.center = std::array::from_fn(|i| self.position[i] + offset[i]);
        ensure!(
            self.body.center.iter().all(|v| v.is_finite()),
            "battle center overflow"
        );
        Ok(())
    }
}

pub(crate) fn rotate([x, y, z]: [f32; 3], heading: f32) -> [f32; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    // PSMTXMultVec adds paired (x,z) and (y,translation) products. Keep that
    // order, including the zero translation, for rounding and signed zero.
    [[cos, 0., sin], [0., 1., 0.], [-sin, 0., cos]]
        .map(|[a, b, c]| c.mul_add(z, a * x) + (b * y + 0.))
}

/// 3F0C0 modifier9: Y * X * Z matrices, then the radius on local +X.
/// Keep SDK matrix concatenation's multiply, Y-fused, Z-fused order.
pub(crate) fn polar_point(angles: [f32; 3], radius: f32) -> [f32; 3] {
    let [[sx, cx], [sy, cy], [sz, cz]] = angles.map(|angle| {
        let (sin, cos) = angle.to_radians().sin_cos();
        [sin, cos]
    });
    let x = [[1., 0., 0.], [0., cx, -sx], [0., sx, cx]];
    let y = [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]];
    let z = [[cz, -sz, 0.], [sz, cz, 0.], [0., 0., 1.]];
    let concat = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| {
        std::array::from_fn::<_, 3, _>(|row| {
            std::array::from_fn::<_, 3, _>(|column| {
                a[row][2].mul_add(
                    b[2][column],
                    a[row][1].mul_add(b[1][column], a[row][0] * b[0][column]),
                )
            })
        })
    };
    concat(y, concat(x, z)).map(|row| row[2].mul_add(0., row[0] * radius) + (row[1] * 0. + 0.))
}

/// The five original tests are inclusive. GroundCircle tests actor-root Y;
/// Ring uses unscaled radial bounds but a scaled vertical extent.
pub(crate) fn overlaps(
    shape: HitShape,
    dimensions: [f32; 2],
    center: [f32; 3],
    attack_scale: f32,
    point: HurtPoint,
    target_scale: f32,
    target_y: f32,
) -> Result<bool> {
    let [radius, height] = dimensions;
    let hurt_radius = point.radius * target_scale;
    let horizontal = radius.mul_add(attack_scale, hurt_radius);
    let vertical = height.mul_add(attack_scale, hurt_radius);
    ensure!(
        horizontal.is_finite() && vertical.is_finite(),
        "actor contact bounds overflow"
    );
    let delta: [f32; 3] = std::array::from_fn(|i| point.center[i] - center[i]);
    Ok(match shape {
        HitShape::Box => {
            delta[0].abs() <= horizontal
                && delta[1].abs() <= vertical
                && delta[2].abs() <= horizontal
        }
        HitShape::Cylinder => delta[1].abs() <= vertical && planar(delta) <= horizontal,
        HitShape::GroundCircle => target_y <= 0.1 && planar(delta) <= horizontal,
        HitShape::Ring { width } => {
            if delta[1].abs() > vertical {
                return Ok(false);
            }
            let distance = planar(delta);
            let end = radius + width;
            ensure!(end.is_finite(), "actor ring bounds overflow");
            distance >= radius.min(end) - hurt_radius && distance <= radius.max(end) + hurt_radius
        }
        HitShape::Sphere => crate::distance::length(delta) <= horizontal,
    })
}

fn planar([x, _, z]: [f32; 3]) -> f32 {
    crate::distance::length([x, 0., z])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_center_scales_before_yaw_and_rejects_overflow() -> Result<()> {
        let mut actor = crate::tests::actor(crate::Side::Party);
        actor.body.center_offset = [10., 80., -5.];
        actor.body.scale = 2.;
        actor.position = [100., 20., -30.];
        actor.heading = 90.;
        actor.sample_center()?;
        assert_eq!(actor.body.center, [90., 180., -50.]);
        assert_eq!(actor.body.audio_position, [100., 0., -30.]);
        actor.position[0] += 7.;
        assert_eq!(actor.body.audio_position, [100., 0., -30.]);
        actor.body.center_offset[0] = f32::MAX;
        assert!(actor.sample_center().is_err());
        Ok(())
    }

    #[test]
    fn hurt_shape_decisions_match_original_dolphin_branches() {
        let trace: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-geometry.json")).unwrap();
        for row in trace["observations"].as_array().unwrap() {
            let float = |name: &str| f32::from_bits(row[name].as_u64().unwrap() as u32);
            let vector = |name: &str| {
                std::array::from_fn(|i| f32::from_bits(row[name][i].as_u64().unwrap() as u32))
            };
            let shape = match row["shape"].as_u64().unwrap() {
                0 => HitShape::Box,
                1 => HitShape::Cylinder,
                2 => HitShape::GroundCircle,
                3 => HitShape::Ring {
                    width: float("ring_width_bits"),
                },
                4 => HitShape::Sphere,
                kind => panic!("unexpected original hit shape {kind}"),
            };
            let result = overlaps(
                shape,
                [float("radius_bits"), float("height_bits")],
                vector("center_bits"),
                float("attack_scale_bits"),
                HurtPoint {
                    center: vector("point_bits"),
                    radius: row["hurt_radius"].as_f64().unwrap() as f32,
                },
                float("target_scale_bits"),
                float("target_y_bits"),
            )
            .unwrap();
            assert_eq!(
                result,
                row["overlaps"].as_bool().unwrap(),
                "original geometry test {}",
                row["index"]
            );
        }
    }

    fn check(shape: HitShape, center: [f32; 3], root_y: f32) -> bool {
        overlaps(
            shape,
            [2., 1.],
            [0.; 3],
            2.,
            HurtPoint { center, radius: 1. },
            2.,
            root_y,
        )
        .unwrap()
    }

    #[test]
    fn box_cylinder_and_sphere_use_scaled_extents_and_inclusive_bounds() {
        assert!(check(HitShape::Box, [6., 4., 6.], 0.));
        assert!(!check(HitShape::Box, [6.001, 4., 6.], 0.));
        assert!(!check(HitShape::Cylinder, [6., 0., 6.], 0.));
        assert!(check(HitShape::Cylinder, [0., 4., 6.], 0.));
        assert!(!check(HitShape::Cylinder, [0., 4.001, 6.], 0.));
        assert!(check(HitShape::Sphere, [0., 6., 0.], 0.));
        assert!(!check(HitShape::Sphere, [0., 4., 6.], 0.));
    }

    #[test]
    fn ground_circle_checks_root_height_instead_of_the_hurt_point_height() {
        assert!(check(HitShape::GroundCircle, [0., 1000., 6.], 0.1));
        assert!(!check(HitShape::GroundCircle, [0., 0., 6.], 0.10001));
        assert!(!check(HitShape::GroundCircle, [0., 0., 6.001], 0.));
    }

    #[test]
    fn rings_keep_unscaled_radial_bounds_and_allow_negative_width() {
        let hit = |width, x, y| {
            overlaps(
                HitShape::Ring { width },
                [10., 1.],
                [0.; 3],
                3.,
                HurtPoint {
                    center: [x, y, 0.],
                    radius: 1.,
                },
                2.,
                0.,
            )
            .unwrap()
        };
        assert!(hit(-4., 5., 5.)); // [6, 10] expanded by hurt radius 2, height 3+2.
        assert!(!hit(-4., 3., 0.));
        assert!(!hit(-4., 13., 0.));
        assert!(!hit(-4., 5., 5.001));
        assert!(hit(4., 9., 0.)); // [10, 14] expanded by hurt radius 2.
        assert!(!hit(4., 17., 0.));
        assert!(!hit(4., 30., 0.)); // Attacker scale never moves the ring to radius 30.
    }
}
