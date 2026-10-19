//! Entry positions in formation order, grouped by each member's strategy row.
use super::Spawn;
use anyhow::{Context, Result, ensure};
use resonance_content::{battle_enemy::EntryRow, menu_data::MenuData};
use resonance_events::party::Party;

fn party_rows(lanes: &[usize]) -> Vec<[f32; 2]> {
    const ROW_X: [f32; 3] = [-300., -500., -700.];
    const LEADER_ROW_Z: [f32; 4] = [0., -250., 250., -450.];
    const SINGLE_MEMBER_Z: [f32; 3] = [150., -150., 150.];
    const DEPTH_STAGGER: f32 = 50.;
    const ROW_WIDTH_SPACING: f32 = 400.;
    lanes
        .iter()
        .enumerate()
        .map(|(index, &lane)| {
            let ordinal = lanes[..index].iter().filter(|&&row| row == lane).count();
            let count = lanes.iter().filter(|&&row| row == lane).count();
            let z = if lane == lanes[0] {
                LEADER_ROW_Z[ordinal]
            } else if count == 1 {
                SINGLE_MEMBER_Z[lane]
            } else {
                ROW_WIDTH_SPACING * (0.5 - ordinal as f32)
            };
            [ROW_X[lane] - DEPTH_STAGGER * ordinal as f32, z]
        })
        .collect()
}

fn enemy_rows(lanes: &[usize]) -> Vec<[f32; 2]> {
    const ROW_X: [f32; 3] = [100., 350., 600.];
    const DEPTH_STAGGER: [f32; 3] = [50., 50., -50.];
    const LEADING_ROW_Z: [f32; 8] = [0., -250., 250., -450., -650., 450., 650., -800.];
    const SINGLE_MEMBER_Z: [f32; 3] = [150., -150., 150.];
    const MEMBER_WIDTH_SPACING: f32 = 250.;
    const GROUP_CENTERING_STEP: f32 = 200.;
    // Large rosters occupy a compact grid instead of extending a row beyond
    // the arena. Normal formations keep their authored row arrangement.
    if lanes.len() > LEADING_ROW_Z.len() {
        let depth = (lanes.len() as f32).sqrt().ceil() as usize;
        let width = lanes.len().div_ceil(depth);
        const GRID_FRONT: f32 = 100.;
        const GRID_BACK: f32 = 700.;
        const GRID_WIDTH: f32 = 800.;
        let depth_step = ((GRID_BACK - GRID_FRONT) / (depth - 1) as f32).min(MEMBER_WIDTH_SPACING);
        let width_step = (GRID_WIDTH / (width - 1) as f32).min(MEMBER_WIDTH_SPACING);
        return (0..lanes.len())
            .map(|i| {
                [
                    GRID_FRONT + (i / width) as f32 * depth_step,
                    ((i % width) as f32 - (width - 1) as f32 * 0.5) * width_step,
                ]
            })
            .collect();
    }
    let leading_lane = lanes.iter().copied().min();
    lanes
        .iter()
        .enumerate()
        .map(|(index, &lane)| {
            let count = lanes.iter().filter(|&&other| other == lane).count();
            let ordinal = lanes[..index]
                .iter()
                .filter(|&&other| other == lane)
                .count();
            let z = if Some(lane) == leading_lane {
                LEADING_ROW_Z[ordinal]
            } else if count == 1 {
                SINGLE_MEMBER_Z[lane]
            } else {
                GROUP_CENTERING_STEP * (count / 2) as f32 - MEMBER_WIDTH_SPACING * ordinal as f32
            };
            [ROW_X[lane] + DEPTH_STAGGER[lane] * ordinal as f32, z]
        })
        .collect()
}

pub(super) fn party_positions(menus: &MenuData, party: &Party) -> Result<Vec<[f32; 3]>> {
    ensure!(!party.formation.is_empty(), "empty battle formation");
    let lanes = party
        .formation
        .iter()
        .take(4)
        .map(|&character| {
            let index = usize::from(
                character
                    .checked_sub(1)
                    .context("invalid party character")?,
            );
            let member = party.members.get(index).context("missing party member")?;
            ensure!(
                index < menus.strategy.default_positions.len(),
                "missing character battle position strategy"
            );
            ensure!(member.strategy[2] < 7, "invalid battle position strategy");
            let lane = menus.strategy.lane(index, member.strategy[2]);
            ensure!(lane < 3, "invalid party lane");
            Ok(lane)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(party_rows(&lanes)
        .into_iter()
        .map(|[x, z]| [x, 0., z])
        .collect())
}

pub(super) fn enemy_positions(
    spawns: &[Spawn],
    choices: &[EntryRow],
    mut roll: impl FnMut() -> u16,
) -> Result<Vec<[f32; 2]>> {
    ensure!(
        !spawns.is_empty() && spawns.len() == choices.len(),
        "invalid enemy placement roster"
    );
    let automatic = spawns[0].position.is_none();
    ensure!(
        spawns
            .iter()
            .all(|spawn| spawn.position.is_none() == automatic),
        "mixed automatic and explicit enemy placement"
    );
    if !automatic {
        // Keep explicit entry positions; movement applies arena constraints.
        return Ok(spawns
            .iter()
            .map(|spawn| spawn.position.unwrap().map(f32::from))
            .collect());
    }
    let lanes: Vec<_> = choices
        .iter()
        .map(|choice| match choice {
            EntryRow::Random => usize::from(roll() % 3),
            EntryRow::Front => 0,
            EntryRow::Middle => 1,
            EntryRow::Back => 2,
        })
        .collect();
    Ok(enemy_rows(&lanes))
}

#[cfg(test)]
mod tests;
