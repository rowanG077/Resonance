//! Actor contact geometry.
use anyhow::{Result, ensure};

pub use resonance_content::battle_projectile::HitShape;

/// One upright capsule in actor-local units. Animation never changes its shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collider {
    pub radius: f32,
    pub half_height: f32,
    pub center_height: f32,
}

impl Collider {
    pub const fn standing(radius: f32, height: f32) -> Self {
        Self {
            radius,
            half_height: height / 2. - radius,
            center_height: height / 2.,
        }
    }

    pub const fn sphere(radius: f32) -> Self {
        Self {
            radius,
            half_height: 0.,
            center_height: 0.,
        }
    }

    fn validate(self) -> bool {
        self.radius.is_finite()
            && self.radius >= 0.
            && self.half_height.is_finite()
            && self.half_height >= 0.
            && self.center_height.is_finite()
    }
}

/// A sphere on the capsule axis, nearest to an incoming contact's height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HurtPoint {
    pub center: [f32; 3],
    pub radius: f32,
}

/// Actor contact geometry and effect origins.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub scale: f32,
    /// Effect origin in actor-local units.
    pub center_offset: [f32; 3],
    /// None is an actor without a contact body.
    pub collider: Option<Collider>,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            scale: 1.,
            center_offset: [0.; 3],
            collider: None,
        }
    }
}

impl Body {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.scale.is_finite()
                && self.scale > 0.
                && self.center_offset.iter().all(|v| v.is_finite())
                && self.collider.is_none_or(|c| c.validate()
                    && [c.radius, c.half_height, c.center_height]
                        .iter()
                        .all(|v| (v * self.scale).is_finite())),
            "invalid battle body"
        );
        Ok(())
    }
}

impl crate::Actor {
    pub fn body_radius(&self) -> f32 {
        self.body
            .collider
            .map_or(0., |collider| collider.radius * self.body.scale)
    }

    pub fn body_top(&self) -> f32 {
        self.position[1]
            + self.body.collider.map_or(0., |collider| {
                (collider.center_height + collider.half_height + collider.radius) * self.body.scale
            })
    }

    pub(crate) fn hurt_point(&self, height: f32) -> Option<HurtPoint> {
        let collider = self.body.collider?;
        let center = self.position[1] + collider.center_height * self.body.scale;
        let half_height = collider.half_height * self.body.scale;
        Some(HurtPoint {
            center: [
                self.position[0],
                height.clamp(center - half_height, center + half_height),
                self.position[2],
            ],
            radius: collider.radius,
        })
    }

    /// Scale and rotate an actor-local point around the current root.
    pub(crate) fn local_point(&self, offset: [f32; 3]) -> [f32; 3] {
        let offset = rotate(offset.map(|v| v * self.body.scale), self.heading);
        std::array::from_fn(|i| self.position[i] + offset[i])
    }

    pub fn effect_origin(&self) -> [f32; 3] {
        self.local_point(self.body.center_offset)
    }

    pub fn audio_position(&self) -> [f32; 3] {
        [self.position[0], 0., self.position[2]]
    }

    pub fn target_center(&self) -> [f32; 3] {
        let mut center = self.position;
        center[1] += self
            .body
            .collider
            .map_or(0., |c| c.center_height * self.body.scale);
        center
    }
}

pub(crate) fn rotate([x, y, z]: [f32; 3], heading: f32) -> [f32; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    [cos * x + sin * z, y, cos * z - sin * x]
}

/// Rotate a radius vector around Z, then X, then Y.
pub(crate) fn polar_point(angles: [f32; 3], radius: f32) -> [f32; 3] {
    let (sx, cx) = angles[0].to_radians().sin_cos();
    let (sz, cz) = angles[2].to_radians().sin_cos();
    rotate([radius * cz, radius * sz * cx, radius * sz * sx], angles[1])
}

/// All shape tests include their boundaries. GroundCircle tests root height; Ring scales only
/// its vertical extent.
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
    fn standing_collider_covers_the_body_and_follows_the_actor_without_a_model() -> Result<()> {
        let mut actor = crate::tests::actor(crate::Side::Enemy);
        actor.body.collider = Some(Collider::standing(20., 120.));
        let touches = |actor: &crate::Actor, at: [f32; 3]| {
            overlaps(
                HitShape::Sphere,
                [1., 0.],
                at,
                1.,
                actor.hurt_point(at[1]).unwrap(),
                actor.body.scale,
                actor.position[1],
            )
        };
        for height in [0., 60., 120.] {
            assert!(touches(&actor, [0., height, 0.])?);
        }
        assert!(!touches(&actor, [50., 60., 0.])?);
        actor.position = [200., 50., 0.];
        assert!(!touches(&actor, [0., 60., 0.])?);
        assert!(touches(&actor, [200., 110., 0.])?);

        assert_eq!(actor.target_center(), [200., 110., 0.]);
        Ok(())
    }

    #[test]
    fn derived_points_follow_the_live_root_and_reject_invalid_preparation() {
        let mut actor = crate::tests::actor(crate::Side::Party);
        actor.body.center_offset = [10., 80., -5.];
        actor.body.scale = 2.;
        actor.position = [100., 20., -30.];
        actor.heading = 90.;

        assert_eq!(actor.effect_origin(), [90., 180., -50.]);
        assert_eq!(actor.audio_position(), [100., 0., -30.]);
        actor.position[0] += 7.;
        assert_eq!(actor.effect_origin(), [97., 180., -50.]);
        assert_eq!(actor.audio_position(), [107., 0., -30.]);
        actor.body.center_offset[0] = f32::MAX;
        assert!(actor.validate().is_err());
    }

    #[test]
    fn rotations_preserve_radius_and_follow_the_named_axes() {
        let close = |actual: [f32; 3], expected: [f32; 3]| {
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.0001);
            }
        };
        close(rotate([2., 3., 0.], 90.), [0., 3., -2.]);
        close(polar_point([0., 90., 0.], 2.), [0., 0., -2.]);
        close(polar_point([0., 0., 90.], 2.), [0., 2., 0.]);
        close(polar_point([90., 0., 90.], 2.), [0., 0., 2.]);
        for angles in [[13., 27., 41.], [-180., 720., 63.]] {
            let point = polar_point(angles, 7.);
            let length = point
                .into_iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt();
            assert!((length - 7.).abs() < 0.0001);
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
