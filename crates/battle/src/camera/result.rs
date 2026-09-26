//! The result callback owns these visits; ordinary camera updates stay paused.
use super::CameraPose;
use crate::{ActorId, Battle, BattlePhase, PreparedBattle, Side};
use anyhow::{Context, Result, ensure};
pub use resonance_content::battle_victory::Camera as ResultCameraParameters;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(super) struct Orbit {
    parameters: ResultCameraParameters,
    minimum_radius: f32,
    angular_step: f32,
    angle: f32,
    radius: f32,
    last_age: Option<u32>,
    participants: u8,
    placed: bool,
}

fn validate(p: ResultCameraParameters) -> Result<()> {
    ensure!(
        [
            p.initial_radius,
            p.minimum_radius,
            p.additional_radius,
            p.pitch,
            p.group_angle,
            p.angular_step,
            p.contraction,
            p.degrees_to_radians,
            p.focus_height,
            p.circle_radius,
            p.circle_extra_degrees,
            p.circle_full_degrees,
        ]
        .into_iter()
        .all(f32::is_finite)
            && p.minimum_radius > 0.
            && p.initial_radius >= p.minimum_radius
            && p.additional_radius >= 0.
            && p.degrees_to_radians > 0.
            && p.contraction >= 0.
            && p.contraction <= p.initial_radius
            && p.circle_radius > 0.
            && p.circle_full_degrees > 0.
            && p.placement
                .iter()
                .flatten()
                .flatten()
                .all(|v| v.is_finite()),
        "invalid result camera parameters"
    );
    Ok(())
}

fn layout(
    parameters: ResultCameraParameters,
    angle: f32,
    participants: &[ActorId],
    party: &[ActorId],
) -> Result<Vec<(ActorId, [f32; 3], f32)>> {
    let party_set: BTreeSet<_> = party.iter().copied().collect();
    let participants_set: BTreeSet<_> = participants.iter().copied().collect();
    ensure!(
        (1..=4).contains(&party.len())
            && !participants.is_empty()
            && party.len() == party_set.len()
            && participants.len() == participants_set.len()
            && participants_set.is_subset(&party_set),
        "invalid result placement roster"
    );
    if participants.len() == 1 {
        let step = if party.len() >= 2 {
            parameters.circle_extra_degrees
                + parameters.circle_full_degrees / (party.len() - 1) as f32
        } else {
            0.
        };
        let mut other = 0;
        Ok(party
            .iter()
            .map(|&id| {
                let position = if id == participants[0] {
                    [0.; 3]
                } else {
                    let radians = f64::from((other as f32 * step) * parameters.degrees_to_radians);
                    other += 1;
                    [
                        parameters.circle_radius * radians.cos() as f32,
                        0.,
                        parameters.circle_radius * radians.sin() as f32,
                    ]
                };
                (id, position, angle)
            })
            .collect())
    } else {
        let positions = parameters.placement[participants.len() - 2];
        Ok(participants
            .iter()
            .chain(party.iter().filter(|id| !participants_set.contains(*id)))
            .enumerate()
            .map(|(slot, &id)| (id, positions[slot], 0.))
            .collect())
    }
}

impl Orbit {
    fn pose(&self) -> CameraPose {
        let radians = |degrees: f32| f64::from(degrees * self.parameters.degrees_to_radians);
        let focus = [0., self.parameters.focus_height, 0.];
        // Result orbit uses X=sin and Z=cos; ordinary tracking uses the opposite
        // convention. The authoritative eye/focus is shared by both consumers.
        let direction = [
            radians(self.angle).sin() as f32,
            radians(self.parameters.pitch).sin() as f32,
            radians(self.angle).cos() as f32,
        ];
        CameraPose {
            eye: std::array::from_fn(|i| direction[i] * self.radius + focus[i]),
            focus,
            pitch: self.parameters.pitch,
            yaw: self.angle,
            radius: self.radius,
        }
    }

    fn advance(&mut self, age: u32) -> Result<CameraPose> {
        ensure!(
            age >= 20
                && self
                    .last_age
                    .map_or(age == 20, |previous| previous.checked_add(1) == Some(age)),
            "out-of-order result camera visit"
        );
        // Original 57614 fdivs then57618 fnmsubs: retain the fused subtraction.
        let ratio = self.radius / self.parameters.initial_radius;
        self.radius = (-self.parameters.contraction)
            .mul_add(ratio, self.radius)
            .max(self.minimum_radius);
        self.angle += self.angular_step;
        self.last_age = Some(age);
        Ok(self.pose())
    }
}

impl PreparedBattle {
    pub fn with_result_camera(mut self, parameters: ResultCameraParameters) -> Result<Self> {
        validate(parameters)?;
        self.camera
            .as_mut()
            .context("result orbit requires a prepared camera")?
            .result_parameters = Some(parameters);
        Ok(self)
    }
}

impl Battle {
    /// 57718 commits the initial orbit after rewards, before result base poses.
    /// The single-performer branch consumes its random angle even when the
    /// encounter's stationary override subsequently replaces that angle.
    pub fn begin_result_camera(&mut self, participants: u8, stationary: bool) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results && (1..=4).contains(&participants),
            "invalid result camera initialization"
        );
        let camera = self.camera.as_ref().context("missing result camera")?;
        ensure!(
            camera.result_orbit.is_none(),
            "result camera already initialized"
        );
        let parameters = camera
            .result_parameters
            .context("result camera parameters are not prepared")?;
        let mut angle = if participants == 1 {
            f32::from(self.draw_random() % 3) * 120.
        } else {
            parameters.group_angle
        };
        if stationary {
            angle = 0.;
        }
        let orbit = Orbit {
            parameters,
            minimum_radius: parameters
                .additional_radius
                .mul_add(f32::from(participants - 1), parameters.minimum_radius),
            angular_step: if stationary {
                0.
            } else {
                parameters.angular_step
            },
            angle,
            radius: parameters.initial_radius,
            last_age: None,
            participants,
            placed: false,
        };
        let camera = self.camera.as_mut().unwrap();
        camera.pose = orbit.pose();
        camera.result_orbit = Some(orbit);
        Ok(())
    }

    /// 57718 uses leader then level-up order for ordinary results, or the group
    /// descriptor's character order. Remaining actors retain party roster order.
    /// Placement includes unavailable party members and precedes base poses.
    pub fn place_result_actors(
        &mut self,
        participants: &[ActorId],
        party: &[ActorId],
    ) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "result layout outside results"
        );
        let orbit = self
            .camera
            .as_ref()
            .and_then(|c| c.result_orbit.as_ref())
            .context("uninitialized result camera")?;
        ensure!(
            !orbit.placed
                && orbit.last_age.is_none()
                && participants.len() == usize::from(orbit.participants),
            "invalid result layout visit"
        );
        ensure!(
            party.len() == self.actors.iter().filter(|a| a.side == Side::Party).count(),
            "incomplete result party roster"
        );
        for &id in party {
            ensure!(
                self.actor(id)?.side == Side::Party,
                "result layout includes enemy"
            );
        }
        let placements = layout(orbit.parameters, orbit.angle, participants, party)?;
        for (id, position, heading) in placements {
            self.actors[id.index()].position = position;
            self.actors[id.index()].heading = heading;
            self.target_positions[id.index()] = position;
        }
        self.camera
            .as_mut()
            .unwrap()
            .result_orbit
            .as_mut()
            .unwrap()
            .placed = true;
        Ok(())
    }

    /// Called by the authored result callback, after its shared world visit.
    pub fn advance_result_camera(&mut self, age: u32) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "result camera outside results"
        );
        let camera = self.camera.as_mut().context("missing result camera")?;
        camera.pose = camera
            .result_orbit
            .as_mut()
            .context("uninitialized result camera")?
            .advance(age)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters() -> ResultCameraParameters {
        ResultCameraParameters {
            initial_radius: 2500.,
            minimum_radius: 1400.,
            additional_radius: 40.,
            pitch: 4.5,
            group_angle: -3.,
            angular_step: 0.025,
            contraction: 50.,
            degrees_to_radians: f32::from_bits(0x3c8efa33),
            focus_height: 112.5,
            placement: [
                [
                    [70., 0., 0.],
                    [-70., 0., 0.],
                    [0., 0., -180.],
                    [-140., 0., -180.],
                ],
                [
                    [0., 0., 0.],
                    [-140., 0., 0.],
                    [140., 0., 0.],
                    [-70., 0., -180.],
                ],
                [
                    [55., 0., 0.],
                    [-55., 0., -90.],
                    [165., 0., -90.],
                    [-165., 0., 0.],
                ],
            ],
            circle_radius: 200.,
            circle_extra_degrees: 10.,
            circle_full_degrees: 360.,
        }
    }

    #[test]
    fn result_orbit_uses_callback_ages_fused_decay_and_original_angle_axes() -> Result<()> {
        let mut orbit = Orbit {
            parameters: parameters(),
            minimum_radius: 1400.,
            angular_step: 0.025,
            angle: 0.,
            radius: 2500.,
            last_age: None,
            participants: 1,
            placed: false,
        };
        let pose = orbit.pose();
        assert_eq!(pose.focus, [0., 112.5, 0.]);
        assert_eq!((pose.eye[0], pose.eye[2]), (0., 2500.));
        assert!(orbit.advance(19).is_err());
        let pose = orbit.advance(20)?;
        assert_eq!(pose.radius, 2450.);
        assert_eq!(pose.yaw, 0.025);
        assert!(pose.eye[0] > 0. && pose.eye[2] < 2450.);
        assert!(orbit.advance(20).is_err());
        assert!(orbit.advance(22).is_err());
        for age in 21..=200 {
            orbit.advance(age)?;
        }
        assert_eq!(orbit.pose().radius, 1400.);
        let mut invalid = parameters();
        invalid.initial_radius = 0.;
        assert!(validate(invalid).is_err());
        Ok(())
    }

    #[test]
    fn result_layout_preserves_participant_order_and_places_remaining_party() -> Result<()> {
        let party = [ActorId(0), ActorId(1), ActorId(2)];
        let solo = layout(parameters(), 120., &[party[1]], &party)?;
        assert_eq!(solo[0], (party[0], [200., 0., 0.], 120.));
        assert_eq!(solo[1], (party[1], [0.; 3], 120.));
        assert!(solo[2].1[0] < -190. && solo[2].1[2] < -30.);
        let pair = layout(parameters(), -3., &[party[2], party[0]], &party)?;
        assert_eq!(
            pair,
            vec![
                (party[2], [70., 0., 0.], 0.),
                (party[0], [-70., 0., 0.], 0.),
                (party[1], [0., 0., -180.], 0.),
            ]
        );
        assert!(layout(parameters(), 0., &[party[0], party[0]], &party).is_err());
        assert!(layout(parameters(), 0., &[ActorId(3)], &party).is_err());
        Ok(())
    }
}
