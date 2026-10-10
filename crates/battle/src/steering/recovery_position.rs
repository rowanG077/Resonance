use super::{ARENA_RADIUS, Actor, ActorId, distance, planar_length};

const BODY_SPACING: f32 = 8.;
const RETREAT_DISTANCE: f32 = 150.;

pub(super) fn clearance(actor: &Actor, obstacle: &Actor) -> f32 {
    actor.body_radius() + obstacle.body_radius() + BODY_SPACING
}

pub(super) fn retreat(actor: &Actor, target: &Actor) -> [f32; 3] {
    let away = distance::planar_direction(actor.position, target.position, [-1., 0., 0.]);
    let distance = clearance(actor, target) + RETREAT_DISTANCE;
    [
        actor.position[0] + away[0] * distance,
        0.,
        actor.position[2] + away[2] * distance,
    ]
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
    fn retreat_moves_away_from_target_and_handles_coincident_positions() {
        let owner = body(Side::Party, [0.; 3], 20.);
        let target = body(Side::Enemy, [100., 0., 0.], 40.);
        let destination = retreat(&owner, &target);
        assert!(destination[0] < 0.);
        assert!(planar_length(destination) > clearance(&owner, &target));
        assert!(
            retreat(&owner, &owner)
                .iter()
                .all(|value| value.is_finite())
        );
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
