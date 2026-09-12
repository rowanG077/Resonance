//! Wrap a typed checkpoint with current content identity; never opens a device.
use anyhow::{Context, Result, bail, ensure};
use resonance_content::session::{GameText, SessionData};
use resonance_game::field::FieldCheckpoint;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Deserialize)]
struct ObservedStats {
    id: usize,
    name: String,
    level: u8,
    experience: u32,
    base_stats: [u16; 7],
    hp: u16,
    tp: u16,
    max_hp: u16,
    max_tp: u16,
    luck: u8,
    equipment: [u16; 6],
    title: u8,
    ex_skills: [u8; 4],
}

/// Register observed default-title level gains, without rerolling or changing the source save.
fn register_party_stats(
    checkpoint: &mut FieldCheckpoint,
    root: &Path,
    observation: &Path,
    source_state: &Path,
) -> Result<()> {
    #[derive(Deserialize)]
    struct Observation {
        state_sha256: String,
        party_menu: ObservedParty,
    }
    #[derive(Deserialize)]
    struct ObservedParty {
        formation: Vec<u8>,
        members: Vec<ObservedStats>,
    }
    let observed: Observation = serde_json::from_slice(&fs::read(observation)?)?;
    ensure!(
        observed.state_sha256 == format!("{:x}", Sha256::digest(fs::read(source_state)?)),
        "party observations belong to a different source state"
    );
    let data: SessionData =
        serde_json::from_slice(&fs::read(root.join("game/session-data.json"))?)?;
    let text: GameText = serde_json::from_slice(&fs::read(root.join("game/text.json"))?)?;
    data.validate()?;
    let party = &mut checkpoint.progress.party;
    ensure!(
        observed.party_menu.formation == party.formation
            && observed.party_menu.members.len() == data.characters.len()
            && party.members.len() == data.characters.len(),
        "source party identity differs"
    );
    for (index, ((member, definition), observed)) in party
        .members
        .iter_mut()
        .zip(&data.characters)
        .zip(observed.party_menu.members)
        .enumerate()
    {
        ensure!(
            observed.id == index + 1
                && Some(&observed.name)
                    == member
                        .name
                        .as_ref()
                        .or_else(|| text.characters.get(&(observed.id as i32)))
                && observed.level == member.level
                && observed.experience == member.experience
                && observed.equipment == member.equipment
                && observed.title == member.title
                && observed.title == 1
                && observed.ex_skills == [0; 4]
                && member.ex_skills == [0; 4],
            "source member {} identity, level or loadout differs",
            observed.id
        );
        let levels = u32::from(
            observed
                .level
                .checked_sub(definition.level)
                .context("source level precedes initial character level")?,
        );
        for (stat, cap) in [9999u32, 999, 32767, 32767, 32767, 32767, 32767]
            .into_iter()
            .enumerate()
        {
            let growth = &definition.growth[stat];
            let minimum = u32::from(definition.base_stats[stat])
                + levels * (u32::from(growth.base) + u32::from(growth.title_bonus));
            let maximum = minimum + levels * u32::from(growth.random);
            ensure!(
                (minimum.min(cap)..=maximum.min(cap))
                    .contains(&u32::from(observed.base_stats[stat])),
                "source member {} stat {stat} is outside initial-title growth bounds",
                observed.id
            );
        }
        ensure!(
            [observed.max_hp, observed.max_tp] == observed.base_stats[..2]
                && observed.hp <= observed.max_hp
                && observed.tp <= observed.max_tp
                && observed.luck < 100,
            "source member {} has invalid unmodified vitals or luck",
            observed.id
        );
        member.base_stats = observed.base_stats;
        member.hp = observed.hp;
        member.tp = observed.tp;
        member.luck = observed.luck;
    }
    party.validate(&data)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .context("INPUT.json OUTPUT.json COOKED_ROOT [OPTIONS]")?;
    let output = args.next().context("OUTPUT.json")?;
    let root = args.next().context("COOKED_ROOT")?;
    let input: serde_json::Value = serde_json::from_slice(&fs::read(input)?)?;
    let mut checkpoint: FieldCheckpoint =
        serde_json::from_value(input.get("state").unwrap_or(&input).clone())?;
    let original_map = checkpoint.map_id;
    let mut camera_supplied = false;
    while let Some(option) = args.next() {
        let mut value = || {
            args.next()
                .with_context(|| format!("{option} requires a value"))
        };
        match option.as_str() {
            "--map-id" => checkpoint.map_id = value()?.parse()?,
            "--camera-settings" => {
                checkpoint.camera = Some(serde_json::from_slice(&fs::read(value()?)?)?);
                camera_supplied = true;
            }
            "--story" => {
                *checkpoint
                    .progress
                    .script_globals
                    .get_mut(16)
                    .context("missing story")? = value()?.parse()?;
            }
            "--position" => {
                checkpoint.position = [value()?.parse()?, value()?.parse()?, value()?.parse()?]
            }
            "--heading" => checkpoint.heading = value()?.parse()?,
            "--party-stats" => {
                let observation = value()?;
                let source_state = value()?;
                register_party_stats(
                    &mut checkpoint,
                    Path::new(&root),
                    Path::new(&observation),
                    Path::new(&source_state),
                )?;
            }
            "--set-event" => {
                checkpoint.progress.event_flags.insert(value()?.parse()?);
            }
            "--clear-event" => {
                checkpoint.progress.event_flags.remove(&value()?.parse()?);
            }
            _ => bail!("unknown fixture option {option}"),
        }
    }
    if checkpoint.map_id != original_map && !camera_supplied {
        bail!("changing the field requires observed --camera-settings");
    }
    resonance_presentation::prepare_checkpoint_fixture(
        Path::new(&root),
        checkpoint,
        Path::new(&output),
    )
}
