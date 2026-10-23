//! Field exits place the player outside an authored world landmark.
use super::{
    Position, World,
    landmarks::Locations,
    travel::{Mount, State},
};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{EntryPlacement, Interaction, MovementParameters};
use std::f32::consts::TAU;

pub(super) fn resolve(
    locations: &Locations,
    parameters: &MovementParameters,
    previous: Option<&State>,
    id: u16,
    direction: i16,
) -> Result<State> {
    if id == 0 {
        return previous
            .cloned()
            .context("world resume has no saved location");
    }
    ensure!(id < 513, "world cinematic {id} requires its scripted scene");
    let landmark = locations
        .definition(id)
        .context("world return landmark is missing")?;
    let world = if id < 256 {
        World::Sylvarant
    } else {
        World::TetheAlla
    };
    let mut state = previous.cloned().unwrap_or(State {
        world,
        position: Position::from_map([0.; 3])?,
        heading: 0.,
        camera_yaw: 0.,
        alternate_perspective: false,
        map_display: Default::default(),
        mount: Mount::Foot,
        altitude: 0.,
    });
    state.world = world;
    let rule = landmark.entry_overrides.iter().find(|rule| {
        rule.direction
            .is_none_or(|expected| i16::from(expected) == direction)
    });
    match rule
        .map(|rule| rule.placement)
        .unwrap_or(EntryPlacement::Perimeter {
            allow_disabled: false,
        }) {
        EntryPlacement::Fixed { position } => state.position = Position::from_map(position)?,
        EntryPlacement::LandmarkCenter { height } => {
            state.position =
                Position::from_map([landmark.position[0], landmark.position[1], height])?;
        }
        EntryPlacement::Perimeter { allow_disabled } => {
            if allow_disabled
                || locations.appearance(id).unwrap().interaction != Interaction::Disabled
            {
                let radius = (landmark.radius + 100.).trunc();
                let diagonal = (radius / 2.0f32.sqrt()).trunc();
                let (dx, dz, heading) = match direction {
                    0 => (radius, 0., 2),
                    1 => (diagonal, diagonal, 1),
                    2 => (0., radius, 0),
                    3 => (-diagonal, diagonal, 7),
                    4 => (-radius, 0., 6),
                    5 => (-diagonal, -diagonal, 5),
                    6 => (0., -radius, 4),
                    7 => (diagonal, -diagonal, 3),
                    _ => (0., 0., 0),
                };
                state.position =
                    Position::from_map([landmark.position[0] + dx, landmark.position[1] + dz, 0.])?;
                state.heading = quantize(
                    TAU * heading as f32 * 0.125,
                    parameters.heading_quantization,
                );
                state.camera_yaw = quantize(
                    TAU * ((8 - heading) % 8) as f32 * 0.125,
                    parameters.camera_yaw_quantization,
                );
            } else {
                ensure!(
                    previous.is_some(),
                    "disabled world return has no saved location"
                );
            }
        }
    }
    if let Some(rule) = rule {
        if let Some(mount) = rule.mount {
            state.mount = mount;
        }
        if let Some(heading) = rule.heading {
            state.heading = quantize(heading, parameters.heading_quantization);
        }
        if let Some(yaw) = rule.camera_yaw {
            state.camera_yaw = quantize(yaw, parameters.camera_yaw_quantization);
        }
    }
    // World initialization starts flight at 300, then the controller resolves
    // clearance against the ground on its first update (including Exire).
    state.altitude = if state.mount.airborne() {
        parameters.flight_clearance
    } else {
        state.position.map()[2]
    };
    state.validate_shape()?;
    Ok(state)
}
fn quantize(angle: f32, steps: f32) -> f32 {
    (angle * steps).trunc() / steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::{
        landmarks::tests::definitions,
        travel::tests::{parameters, state},
    };
    use std::sync::Arc;

    fn entry_definitions() -> resonance_content::overworld::Landmarks {
        let mut data = definitions();
        for (id, json) in [
            (
                2,
                serde_json::json!({"direction":6,"placement":{"kind":"fixed","position":[9770,24160,0]},"mount":null,"heading":12867./4096.,"camera_yaw":100./32.}),
            ),
            (
                264,
                serde_json::json!({"direction":null,"placement":{"kind":"perimeter","allow_disabled":true},"mount":"ship","heading":null,"camera_yaw":null}),
            ),
            (
                266,
                serde_json::json!({"direction":null,"placement":{"kind":"landmark_center","height":500},"mount":"rheairds","heading":0,"camera_yaw":0}),
            ),
            (
                280,
                serde_json::json!({"direction":null,"placement":{"kind":"fixed","position":[36930,23915,0]},"mount":null,"heading":3216./4096.,"camera_yaw":175./32.}),
            ),
            (
                304,
                serde_json::json!({"direction":null,"placement":{"kind":"landmark_center","height":0},"mount":"rheairds","heading":0,"camera_yaw":0}),
            ),
        ] {
            data.worlds
                .iter_mut()
                .flatten()
                .find(|entry| entry.id == id)
                .unwrap()
                .entry_overrides
                .push(serde_json::from_value(json).unwrap());
        }
        data
    }

    #[test]
    fn exits_use_requested_landmark_direction_and_retain_the_mount() -> Result<()> {
        let mut data = definitions();
        data.worlds[0][0].position = [1000., 2000.];
        data.worlds[0][0].radius = 400.;
        data.worlds[0][0].interaction = Interaction::Active;
        let locations = Locations::new(
            Arc::new(data),
            Default::default(),
            crate::overworld::scripts::fixture(),
        )?;
        let previous = state(Mount::Noishe);
        for (direction, expected) in [
            (0, [1500., 2000., 0.]),
            (2, [1000., 2500., 0.]),
            (5, [647., 1647., 0.]),
            (6, [1000., 1500., 0.]),
        ] {
            let next = resolve(&locations, &parameters(), Some(&previous), 1, direction)?;
            assert_eq!(next.position.map(), expected);
            assert_eq!(next.mount, Mount::Noishe);
        }
        assert_eq!(
            resolve(&locations, &parameters(), Some(&previous), 0, 0)?,
            previous
        );
        assert!(resolve(&locations, &parameters(), None, 0, 0).is_err());
        Ok(())
    }

    #[test]
    fn authored_special_exits_keep_their_positions_and_mount_modes() -> Result<()> {
        let locations = Locations::new(
            Arc::new(entry_definitions()),
            Default::default(),
            crate::overworld::scripts::fixture(),
        )?;
        let p = parameters();
        let iselia = resolve(&locations, &p, None, 2, 6)?;
        assert_eq!(iselia.position.map(), [9770., 24160., 0.]);
        assert_eq!(iselia.camera_yaw, 3.125);
        let tower = resolve(&locations, &p, None, 280, 0)?;
        assert_eq!(tower.world, World::TetheAlla);
        assert_eq!(tower.position.map(), [36930., 23915., 0.]);
        let exire = resolve(&locations, &p, None, 266, 0)?;
        assert_eq!(exire.mount, Mount::Rheairds);
        assert_eq!(exire.position.map()[2], 500.);
        assert_eq!(exire.altitude, 300.);
        let bridge = resolve(&locations, &p, None, 264, 0)?;
        assert_eq!(bridge.mount, Mount::Ship);
        Ok(())
    }

    #[test]
    fn entry_overrides_follow_content_and_reject_ambiguous_or_invalid_placement() -> Result<()> {
        let mut data = entry_definitions();
        let mut rule = data.worlds[0][1].entry_overrides.remove(0);
        rule.placement = EntryPlacement::Fixed {
            position: [1111., 2222., 7.],
        };
        rule.mount = Some(Mount::Noishe);
        data.worlds[0][0].entry_overrides.push(rule.clone());
        let locations = Locations::new(
            Arc::new(data.clone()),
            Default::default(),
            crate::overworld::scripts::fixture(),
        )?;
        let previous = state(Mount::Foot);
        let next = resolve(&locations, &parameters(), Some(&previous), 1, 6)?;
        assert_eq!(next.position.map(), [1111., 2222., 7.]);
        assert_eq!(next.mount, Mount::Noishe);
        assert_eq!(next.altitude, 7.);
        // The source landmark no longer has its former special case.
        assert!(resolve(&locations, &parameters(), None, 2, 6).is_err());
        // A nonmatching direction still respects a disabled entrance.
        let retained = resolve(&locations, &parameters(), Some(&previous), 1, 5)?;
        assert_eq!(retained.position, previous.position);
        assert_eq!(retained.mount, previous.mount);
        data.worlds[0][0].entry_overrides.push(rule);
        assert!(
            Locations::new(
                Arc::new(data.clone()),
                Default::default(),
                crate::overworld::scripts::fixture()
            )
            .is_err()
        );
        data.worlds[0][0].entry_overrides.truncate(1);
        data.worlds[0][0].entry_overrides[0].placement =
            EntryPlacement::LandmarkCenter { height: f32::NAN };
        assert!(
            Locations::new(
                Arc::new(data),
                Default::default(),
                crate::overworld::scripts::fixture()
            )
            .is_err()
        );
        Ok(())
    }
}
