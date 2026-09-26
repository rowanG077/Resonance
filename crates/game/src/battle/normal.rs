//! Resource bindings for maintained normal-attack source; selection remains in
//! the shared controller and all timing comes from the verified original table.
use super::{ActionBinding, MeleeResource, MeleeSelection, WeaponFlightResource};
use anyhow::{Context, Result, bail};
use resonance_battle::ActionPhase;
use resonance_content::{
    battle_action::{NORMAL_PATH, NormalTable},
    prepared::Files,
};

pub const ENTRIES: [&str; 7] = [
    "neutral",
    "rising",
    "thrust",
    "low",
    "finisher",
    "aerial_slash",
    "aerial_thrust",
];

pub const LLOYD_ENTRIES: [&str; 7] = ENTRIES;

pub fn lloyd_bindings(files: &Files, ids: [u16; 7]) -> Result<Vec<ActionBinding>> {
    bindings(files, 0, "battle::normal_lloyd", ids)
}

pub fn colette_bindings(files: &Files, ids: [u16; 7]) -> Result<Vec<ActionBinding>> {
    bindings(files, 1, "battle::normal_colette", ids)
}

pub fn genis_bindings(files: &Files, ids: [u16; 7]) -> Result<Vec<ActionBinding>> {
    bindings(files, 2, "battle::normal_genis", ids)
}

fn bindings(
    files: &Files,
    character: usize,
    module: &str,
    ids: [u16; 7],
) -> Result<Vec<ActionBinding>> {
    let table: NormalTable = files.json(NORMAL_PATH)?;
    let group = table
        .groups
        .get(character)
        .context("missing normal-attack character")?;
    ENTRIES
        .iter()
        .zip(ids)
        .enumerate()
        .map(|(selection, (entry, id))| {
            let selector = group
                .selectors
                .get(selection)
                .context("missing normal selector")?;
            let action = group
                .actions
                .get(usize::from(selector.action))
                .context("missing normal action")?;
            let descriptor = group
                .descriptors
                .get(action.descriptor as usize)
                .context("missing normal descriptor")?;
            Ok(ActionBinding {
                id,
                phase: ActionPhase::Actor,
                module: module.into(),
                entry: (*entry).into(),
                duration: descriptor.duration,
                tp_cost: 0,
            })
        })
        .collect()
}

pub fn lloyd_melee(path: &str, anchor_groups: &[Vec<u16>]) -> Result<MeleeResource> {
    let (selection, row) = match path {
        // Neutral and finisher's first contact have identical source parameters.
        "battle/melee/lloyd/right" => (0, 0),
        "battle/melee/lloyd/left" => (4, 1),
        "battle/melee/lloyd/rising" => (1, 0),
        "battle/melee/lloyd/thrust" => (2, 0),
        "battle/melee/lloyd/low" => (3, 0),
        "battle/melee/lloyd/aerial_slash" => (5, 0),
        "battle/melee/lloyd/aerial_thrust" => (6, 0),
        _ => bail!("unknown Lloyd normal contact {path}"),
    };
    Ok(melee(0, selection, row, anchor_groups))
}

pub fn colette_melee(path: &str, anchor_groups: &[Vec<u16>]) -> Result<MeleeResource> {
    let selection = match path {
        "battle/melee/colette/neutral" => 0,
        "battle/melee/colette/rising" => 1,
        "battle/melee/colette/finisher" => 4,
        "battle/melee/colette/aerial_slash" => 5,
        _ => bail!("unknown Colette attached contact {path}"),
    };
    Ok(melee(1, selection, 0, anchor_groups))
}

pub fn genis_melee(path: &str, anchor_groups: &[Vec<u16>]) -> Result<MeleeResource> {
    let entry = path
        .strip_prefix("battle/melee/genis/")
        .context("unknown Genis contact path")?;
    let selection = ENTRIES
        .iter()
        .position(|&name| name == entry)
        .context("unknown Genis normal contact")?;
    Ok(melee(2, selection as u8, 0, anchor_groups))
}

pub fn colette_flight(path: &str) -> Result<WeaponFlightResource> {
    let selection = match path {
        "battle/flights/colette/thrust" => 2,
        "battle/flights/colette/low" => 3,
        "battle/flights/colette/aerial_thrust" => 6,
        _ => bail!("unknown Colette thrown contact {path}"),
    };
    Ok(WeaponFlightResource {
        source: NORMAL_PATH.into(),
        character: 1,
        selection,
        row: 0,
        impact: None,
    })
}

fn melee(character: u8, selection: u8, row: u8, anchor_groups: &[Vec<u16>]) -> MeleeResource {
    MeleeResource {
        impact: None,
        source: NORMAL_PATH.into(),
        selection: MeleeSelection::Normal {
            character,
            selection,
        },
        row,
        anchor_groups: anchor_groups.to_vec(),
    }
}
