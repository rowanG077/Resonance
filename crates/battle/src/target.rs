//! Shared source targeting used during preparation and ordinary decisions.
use crate::Side;
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy)]
pub struct TargetActor {
    pub side: Side,
    pub position: [f32; 3],
    pub available: bool,
    pub hidden: bool,
    pub dying: bool,
    pub target: Option<usize>,
}

/// 36A94/36A48 policies. Random targeting samples the full opposing roster;
/// rejected candidates still consume a draw. Strict comparisons preserve ties.
pub fn select_target(
    actors: &[TargetActor],
    owner: usize,
    policy: u8,
    random: &mut impl FnMut() -> u16,
) -> Result<usize> {
    ensure!(owner < actors.len(), "invalid target owner");
    ensure!(
        actors
            .iter()
            .all(|a| a.target.is_none_or(|target| target < actors.len())
                && a.position.iter().all(|v| v.is_finite())),
        "invalid targeting roster"
    );
    let side = actors[owner].side;
    if policy == 9 {
        return Ok(owner);
    }
    if policy == 3 {
        let leader = actors.iter().position(|a| a.side == side).unwrap();
        return actors[leader]
            .target
            .ok_or_else(|| anyhow::anyhow!("leader target is not initialized"));
    }
    ensure!(matches!(policy, 1 | 4), "unprepared target policy {policy}");
    let opponents: Vec<_> = actors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.side != side)
        .map(|(i, _)| i)
        .collect();
    ensure!(
        !opponents.is_empty(),
        "target policy requires opposing roster"
    );
    let available = opponents.iter().filter(|&&i| actors[i].available).count();
    let eligible =
        |i: usize| actors[i].available && !actors[i].hidden && (available <= 1 || !actors[i].dying);
    if policy == 4 {
        for _ in 0..16 {
            let candidate = opponents[usize::from(random()) % opponents.len()];
            if eligible(candidate)
                && !actors.iter().enumerate().any(|(i, a)| {
                    i != owner && a.side == side && a.available && a.target == Some(candidate)
                })
            {
                return Ok(candidate);
            }
        }
    }
    let mut best = opponents[0];
    let mut distance = 5000.;
    for candidate in opponents {
        if eligible(candidate) {
            let a = actors[owner].position;
            let b = actors[candidate].position;
            // 368F8 reads1084, filled by planar4DBA0 in1C40.
            let value = crate::distance::length([a[0] - b[0], 0., a[2] - b[2]]);
            if value < distance {
                best = candidate;
                distance = value;
            }
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn actor(side: Side, x: f32, target: usize) -> TargetActor {
        TargetActor {
            side,
            position: [x, 0., 0.],
            available: true,
            hidden: false,
            dying: false,
            target: Some(target),
        }
    }
    #[test]
    fn spread_rejects_claimed_targets_and_preserves_sixteen_draw_fallback() {
        let actors = [
            actor(Side::Party, 0., 2),
            actor(Side::Party, 20., 2),
            actor(Side::Enemy, 15., 0),
            actor(Side::Enemy, 30., 0),
        ];
        let mut draws = 0;
        assert_eq!(
            select_target(&actors, 3, 4, &mut || {
                draws += 1;
                0
            })
            .unwrap(),
            1
        );
        assert_eq!(draws, 16);
        draws = 0;
        assert_eq!(
            select_target(&actors, 3, 4, &mut || {
                draws += 1;
                1
            })
            .unwrap(),
            1
        );
        assert_eq!(draws, 1);
    }
    #[test]
    fn leader_policy_uses_live_leader_target_without_a_draw() {
        let actors = [
            actor(Side::Party, 0., 2),
            actor(Side::Party, 10., 0),
            actor(Side::Enemy, 20., 0),
        ];
        assert_eq!(
            select_target(&actors, 1, 3, &mut || panic!("unexpected target draw")).unwrap(),
            2
        );
    }
    #[test]
    fn nearest_policy_uses_original_planar_distance_for_flying_candidates() {
        let mut actors = [
            actor(Side::Party, 0., 1),
            actor(Side::Enemy, 10., 0),
            actor(Side::Enemy, 20., 0),
        ];
        actors[1].position[1] = 1000.;
        assert_eq!(
            select_target(&actors, 0, 1, &mut || panic!("unexpected target draw")).unwrap(),
            1
        );
    }
}
