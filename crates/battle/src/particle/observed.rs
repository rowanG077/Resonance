//! Advance an independently observed first frame through ordinary particle motion.
use super::*;

/// Test support for checking an observed trajectory without replaying its RNG stream.
/// The caller must validate the observed birth against its authored declaration.
/// Texture/color sampling is outside this motion-only observation.
pub struct FixtureParticle(Particle);

impl FixtureParticle {
    pub fn at_first_frame(
        template: ParticleTemplate,
        state: ParticleState,
        origin: [f32; 3],
    ) -> Result<Self> {
        template.validate()?;
        ensure!(
            state.finite()
                && origin.iter().all(|value| value.is_finite())
                && template.lifetime.is_some()
                && !template.follow_origin
                && !template.follow_orientation
                && template.owner_bone.is_none()
                && template.follow_bone.is_none()
                && template.bounce_effect.is_none()
                && template.floor_effect.is_none()
                && template.periodic_effect.is_none(),
            "observed particle requires independent finite motion"
        );
        let fade = template.fade;
        let orbit_velocity = template.orbit_velocity;
        let element_tint = template.element_tint;
        Ok(Self(Particle {
            definition: Arc::new(ParticleDefinition {
                resource: crate::ResourceId(0),
                member: 0,
                model: None,
                data: template,
            }),
            frame: ParticleFrame {
                id: ParticleId(0),
                owner: ActorId(0),
                owner_bone: None,
                draw_after: None,
                resource: crate::ResourceId(0),
                member: 0,
                origin,
                heading: 0.,
                state,
                model: None,
            },
            follow: None,
            orientation_origin: origin,
            target: ActorId(0),
            model: None,
            model_secondary: Vec::new(),
            model_parent: None,
            orbit_velocity,
            element_tint,
            age: 1,
            fade,
            uv_frame: 0,
            uv_elapsed: 0,
            uv_scroll: [0; 2],
            floor_effect_emitted: false,
        }))
    }

    pub fn state(&self) -> &ParticleState {
        &self.0.frame.state
    }

    pub fn advance(&mut self) -> Result<()> {
        ensure!(
            self.0
                .definition
                .data
                .lifetime
                .is_some_and(|life| self.0.age < life),
            "observed particle outlived its declaration"
        );
        ensure!(
            self.0.step(None, None)?.is_none(),
            "unexpected particle callback"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_motion_preserves_bounce_scale_spin_and_lifetime() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../content/tests/fixtures/projectile-model-follow.json"
        ))?;
        let mut template: ParticleTemplate =
            serde_json::from_value(fixture["bank"]["actors"][0]["template"].clone())?;
        template.lifetime = Some(3);
        template.follow_origin = false;
        template.follow_orientation = false;
        template.ground_restitution = Some(0.65);
        template.state.offset = [1., 2., 3.];
        template.state.velocity = [2., -4., 0.];
        template.state.acceleration = [0., -1., 0.];
        template.state.angles = [10., 20., 30.];
        template.state.angular_velocity = [1., 2., 3.];
        template.state.geometry = ParticleGeometry::Size {
            value: [1.; 3],
            velocity: [-0.02; 3],
            acceleration: [0.; 3],
        };
        let mut observed =
            FixtureParticle::at_first_frame(template.clone(), template.state, [0.; 3])?;
        observed.advance()?;
        assert_eq!(observed.state().offset, [3., 0., 3.]);
        assert_eq!(observed.state().velocity, [2., 3.25, 0.]);
        assert_eq!(observed.state().angles, [11., 22., 33.]);
        let ParticleGeometry::Size { value, .. } = observed.state().geometry else {
            unreachable!()
        };
        assert_eq!(value, [0.98; 3]);
        observed.advance()?;
        assert_eq!(observed.state().offset, [5., 3.25, 3.]);
        assert!(observed.advance().is_err());
        Ok(())
    }
}
