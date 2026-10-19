use super::{ARENA_RADIUS, Actor, ActorId, distance, planar_length};
use anyhow::Result;

const BODY_SPACING: f32 = 8.;
const POSITION_CELL_SIZE: f32 = 100.;
const MAX_OCCUPIED_RADIUS: u8 = 3;

pub(super) fn clearance(actor: &Actor, obstacle: &Actor) -> f32 {
    actor.body_radius() + obstacle.body_radius() + BODY_SPACING
}

/// Sample increasingly wider clear positions near the actor, away from its target.
pub(super) fn clear_position(
    actors: &[Actor],
    owner: ActorId,
    target: ActorId,
    mut random: impl FnMut() -> Result<u16>,
) -> Result<Option<[f32; 3]>> {
    let cell = |position: [f32; 3]| {
        [position[0], position[2]].map(|value| (value / POSITION_CELL_SIZE) as i16)
    };
    let origin = cell(actors[owner.index()].position);
    let target = cell(actors[target.index()].position);
    let limit = (ARENA_RADIUS / POSITION_CELL_SIZE) as i16;
    for radius in 1..=limit + 1 {
        let span = (radius + 1) as u16;
        let mut occupied = true;
        for _ in 0..(radius * 2 + 1).pow(2) {
            let sample = [random()?, random()?];
            let candidate = std::array::from_fn::<_, 2, _>(|axis| {
                (origin[axis] + (sample[axis] % span) as i16).clamp(-limit, limit)
            });
            let distance =
                |from: [i16; 2]| (from[0] - candidate[0]).abs() + (from[1] - candidate[1]).abs();
            occupied = actors.iter().any(|actor| {
                if actor.availability == crate::ActorAvailability::Absent {
                    return false;
                }
                let center = cell(actor.position);
                let radius = i16::from(
                    actor
                        .movement
                        .steering
                        .positioning_radius
                        .min(MAX_OCCUPIED_RADIUS),
                );
                (0..2).all(|axis| (center[axis] - candidate[axis]).abs() <= radius)
            });
            if !occupied && distance(target) > distance(origin) {
                return Ok(Some([
                    f32::from(candidate[0]) * POSITION_CELL_SIZE,
                    0.,
                    f32::from(candidate[1]) * POSITION_CELL_SIZE,
                ]));
            }
        }
        // Expand only while the sampled neighborhood remains occupied.
        if !occupied {
            break;
        }
    }
    Ok(None)
}

/// Prefer the requested position, then the nearest clear side of an obstructing body.
/// Returning None leaves the actor in place when the neighborhood is fully occupied.
pub(super) fn select(actors: &[Actor], owner: ActorId, preferred: [f32; 3]) -> Option<[f32; 3]> {
    let actor = &actors[owner.index()];
    let radius = (ARENA_RADIUS - actor.body_radius() - BODY_SPACING).max(0.);
    let clamp = |mut point: [f32; 3]| {
        point[1] = 0.;
        let length = planar_length(point);
        if length > radius {
            point = point.map(|v| v * radius / length);
        }
        point
    };
    let obstacles: Vec<_> = actors
        .iter()
        .enumerate()
        .filter(|(index, other)| {
            *index != owner.index() && other.available() && !other.movement.fixed_height
        })
        .map(|(_, other)| (other, clearance(actor, other)))
        .collect();
    let clear = |point: [f32; 3]| {
        obstacles.iter().all(|(other, radius)| {
            planar_length([
                point[0] - other.position[0],
                0.,
                point[2] - other.position[2],
            ]) >= *radius
        })
    };
    let preferred = clamp(preferred);
    if clear(preferred) {
        return Some(preferred);
    }
    obstacles
        .iter()
        .flat_map(|(other, clearance)| {
            let outward = distance::planar_direction(preferred, other.position, [1., 0., 0.]);
            let normal = [-outward[2], 0., outward[0]];
            [outward, outward.map(|v| -v), normal, normal.map(|v| -v)].map(|direction| {
                clamp([
                    other.position[0] + direction[0] * (clearance + BODY_SPACING),
                    0.,
                    other.position[2] + direction[2] * (clearance + BODY_SPACING),
                ])
            })
        })
        .filter(|&point| clear(point))
        .min_by(|a, b| {
            let gap = |point: &[f32; 3]| {
                planar_length([point[0] - preferred[0], 0., point[2] - preferred[2]])
            };
            gap(a).total_cmp(&gap(b))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorAvailability, Side, tests::actor};

    fn body(side: Side, position: [f32; 3], radius: f32) -> Actor {
        let mut actor = actor(side);
        actor.position = position;
        actor.body.collider = Some(crate::Collider::sphere(radius));
        actor
    }

    #[test]
    fn destination_respects_scaled_bodies_and_arena_edge() {
        let mut owner = body(Side::Party, [0.; 3], 30.);
        owner.body.scale = 2.;
        let obstacle = body(Side::Enemy, [700., 0., 0.], 80.);
        let actors = [owner, obstacle];
        let destination = select(&actors, ActorId(0), [1000., 50., 0.]).unwrap();
        assert_eq!(destination[1], 0.);
        assert!(planar_length(destination) + actors[0].body_radius() <= ARENA_RADIUS);
        assert!(
            planar_length([destination[0] - 700., 0., destination[2]])
                >= clearance(&actors[0], &actors[1])
        );
    }

    #[test]
    fn absent_bodies_do_not_displace_formation_positions() {
        let owner = body(Side::Party, [0.; 3], 20.);
        let mut obstacle = body(Side::Party, [200., 0., 0.], 50.);
        obstacle.availability = ActorAvailability::Absent;
        assert_eq!(
            select(&[owner, obstacle], ActorId(0), [200., 0., 0.]),
            Some([200., 0., 0.])
        );
    }
}
