use resonance_content::field::SLIDING_SURFACE;
use resonance_events::Actor;

pub(super) const SLIDING_SPEED: f32 = 8.;

/// Scripted placement can overlap bodies. Allow separating steps so neither
/// the player nor an NPC remains trapped when normal movement resumes.
pub(super) fn body_blocks_step(
    start: [f32; 3],
    end: [f32; 3],
    obstacle: [f32; 3],
    radius: f32,
) -> bool {
    let offset = [start[0] - obstacle[0], start[1] - obstacle[1]];
    let distance_squared =
        |p: [f32; 3]| (p[0] - obstacle[0]).powi(2) + (p[1] - obstacle[1]).powi(2);
    distance_squared(end) < radius * radius
        && (distance_squared(start) >= radius * radius
            || (end[0] - start[0]) * offset[0] + (end[1] - start[1]) * offset[1] < 0.)
}

/// Carry momentum across sliding ground, including movement from walking onto it.
#[derive(Clone, Default)]
pub(super) struct PlayerMovement {
    instance: Option<u64>,
    position: [f32; 2],
    velocity: [f32; 2],
}

impl PlayerMovement {
    fn velocity(&self, actor: &Actor) -> [f32; 2] {
        if self.instance == Some(actor.instance) && self.position == actor.position[..2] {
            self.velocity
        } else {
            // Actor replacement and authored repositioning discard old momentum.
            [0.; 2]
        }
    }

    pub(super) fn sliding(&self, actor: &Actor) -> bool {
        actor.ground_attributes & SLIDING_SURFACE != 0 && self.velocity(actor) != [0.; 2]
    }

    pub(super) fn delta(&self, actor: &Actor, input: [f32; 2], sliding: bool) -> [f32; 2] {
        if !sliding {
            return input;
        }
        let previous = self.velocity(actor);
        let velocity = if previous == [0.; 2] { input } else { previous };
        let axis = usize::from(velocity[0].abs() <= velocity[1].abs());
        let mut delta = [0.; 2];
        if velocity[axis] != 0. {
            delta[axis] = velocity[axis].signum() * velocity[axis].abs().max(SLIDING_SPEED);
        }
        delta
    }

    pub(super) fn record(&mut self, actor: &Actor, target: [f32; 3]) {
        self.instance = Some(actor.instance);
        self.position = [target[0], target[1]];
        self.velocity = [target[0] - actor.position[0], target[1] - actor.position[1]];
    }
}
