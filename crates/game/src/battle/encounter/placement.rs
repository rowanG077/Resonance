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
    const FRONT_DISTANCE: f32 = 200.;
    const ROW_SPACING: f32 = 140.;
    // Diagonal spacing keeps members in the same lane visibly distinct.
    const MEMBER_SPACING: [f32; 2] = [55., 140.];
    lanes
        .iter()
        .enumerate()
        .map(|(index, &lane)| {
            let count = lanes.iter().filter(|&&other| other == lane).count();
            let column = lanes[..index]
                .iter()
                .filter(|&&other| other == lane)
                .count();
            let offset = column as f32 - (count - 1) as f32 / 2.;
            [
                FRONT_DISTANCE + lane as f32 * ROW_SPACING + offset * MEMBER_SPACING[0],
                offset * MEMBER_SPACING[1],
            ]
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
        !spawns.is_empty()
            && spawns.len() <= resonance_battle::ENEMY_CAPACITY
            && spawns.len() == choices.len(),
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
