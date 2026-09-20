//! Select enemy programs from the complete cooked tables.
use super::*;
use crate::battle::{
    all::{ActorSettings, EnemyResources},
    animation_table,
};
use crate::cooked::Source;
use serde::Deserialize;

#[derive(Deserialize)]
struct Rows {
    actions: Vec<EnemyActionRecord>,
}

#[derive(Deserialize)]
struct Rules {
    records: Vec<HitRuleRecord>,
}

#[derive(Deserialize)]
struct Hits {
    entries: Vec<HitEntry>,
}

#[derive(Deserialize)]
struct HitEntry {
    index: usize,
    record: HitRecord,
}

#[derive(Deserialize)]
pub(super) struct Commands {
    entries: Vec<CommandEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CommandEntry {
    End {
        offset: usize,
        end: bool,
        loops: bool,
    },
    Step {
        offset: usize,
        #[serde(flatten)]
        command: crate::battle::action_program::Command,
    },
}

impl Commands {
    pub fn select(&self, index: i16) -> Result<(Vec<TimedCommand>, bool)> {
        let mut offset = usize::try_from(index).context("negative enemy command index")? * 2;
        let mut result = Vec::new();
        let start =
            self.entries
                .iter()
                .position(|entry| match entry {
                    CommandEntry::End { offset: at, .. }
                    | CommandEntry::Step { offset: at, .. } => *at == offset,
                })
                .context("enemy command index outside table")?;
        for entry in self.entries.iter().skip(start).take(LIMIT) {
            match entry {
                CommandEntry::End {
                    offset: at,
                    end,
                    loops,
                } => {
                    ensure!(*at == offset && *end, "invalid enemy command terminator");
                    return Ok((result, *loops));
                }
                CommandEntry::Step {
                    offset: at,
                    command,
                } => {
                    ensure!(*at == offset, "noncontiguous enemy command records");
                    result.push(lower_command(command)?);
                    offset += 4 + command.operands.len() * 2;
                }
            }
        }
        bail!("enemy command program has no bounded terminator")
    }
}

#[derive(Deserialize)]
pub(super) struct Policy {
    native_policy: u8,
    ordinary_action_count: u8,
    back_row_count: u8,
    back_row: Vec<EnemyBackRow>,
    overlimit_first_action: bool,
}

pub(super) struct Records {
    pub settings: ActorSettings,
    pub resources: EnemyResources,
    pub rows: Vec<EnemyActionRecord>,
    pub commands: Commands,
    rules: Vec<HitRuleRecord>,
    hits: BTreeMap<usize, HitRecord>,
    animations: animation_table::Parsed,
}

fn table<T: serde::de::DeserializeOwned>(
    source: &Source<'_>,
    monster: u8,
    header: u16,
) -> Result<T> {
    let (_, bytes) = source.resolve(&format!(
        "battle/all/enemy-{monster}/header-{header:x}.json"
    ))?;
    Ok(serde_json::from_slice(&bytes)?)
}

impl Records {
    pub fn bind(source: &Source<'_>, monster: u8) -> Result<Self> {
        Ok(Self {
            settings: table(source, monster, 4)?,
            resources: table(source, monster, 0x14)?,
            rows: table::<Rows>(source, monster, 10)?.actions,
            commands: table(source, monster, 14)?,
            rules: table::<Rules>(source, monster, 8)?.records,
            hits: table::<Hits>(source, monster, 16)?
                .entries
                .into_iter()
                .map(|entry| (entry.index, entry.record))
                .collect(),
            animations: table(source, monster, 18)?,
        })
    }

    pub fn motion_absent(&self, clip: u8) -> Result<bool> {
        Ok(*self
            .resources
            .motion_offsets
            .get(usize::from(clip))
            .context("missing enemy motion declaration")?
            == 0)
    }

    pub fn hits(&self, root: u16) -> Result<Vec<HitWindow>> {
        let root = usize::try_from(root as i16).context("negative enemy hit index")?;
        let mut hits = Vec::new();
        for index in root..root + LIMIT {
            let record = self
                .hits
                .get(&index)
                .context("enemy hit index outside table")?;
            match record.lower_with(|rule| {
                self.rules
                    .get(usize::from(rule))
                    .context("invalid hit rule index")?
                    .lower()
            })? {
                Some(hit) => hits.push(hit),
                None => return Ok(hits),
            }
        }
        bail!("hit program exceeds bounded record limit")
    }

    pub fn animations(&self, root: u16) -> Result<AnimationProgram> {
        let root = usize::try_from(root as i8).context("negative enemy animation root")?;
        self.animations.selected(root * ANIMATION_BYTES)
    }

    pub fn policy(
        &self,
        source: &Source<'_>,
        monster: u8,
        parameters: &enemy::Parameters,
        lengths: &enemy::VoiceDurations,
    ) -> Result<EnemyPolicy> {
        let table: Policy = table(source, monster, 12)?;
        ensure!(
            usize::from(table.ordinary_action_count) <= self.rows.len()
                && table.back_row_count <= 4
                && table.back_row.len() == 4,
            "invalid enemy AI action counts"
        );
        let back_row = table
            .back_row
            .into_iter()
            .take(usize::from(table.back_row_count))
            .collect::<Vec<_>>();
        ensure!(
            back_row
                .iter()
                .all(|row| usize::from(row.action) < self.rows.len()),
            "invalid back-row action"
        );
        Ok(EnemyPolicy {
            native: Some(parameters.bind(table.native_policy, &self.settings, lengths)?),
            ordinary_actions: table.ordinary_action_count,
            back_row,
            overlimit_first_action: table.overlimit_first_action,
            occupies_front_row: self.settings.model.body_flags & 0x24 != 0x24,
            close_distance: parameters.close_distance,
        })
    }

    #[cfg(test)]
    pub fn read(bytes: &[u8]) -> Result<Self> {
        use crate::battle::{action_program, enemy_inventory};
        ensure!(bytes.starts_with(b"em8\0"), "invalid enemy action package");
        let section =
            |header| enemy_inventory::offset_section(bytes, usize::from(half(bytes, header)?));
        let rows = EnemyActionRecord::table(section(10)?)?;
        let hit_pool = section(16)?;
        let hit_extent = enemy_inventory::program_extent(hit_pool, HIT_BYTES, -1)?;
        let animation_pool = section(18)?;
        let roots = rows
            .iter()
            .map(|row| Ok(usize::try_from(row.animation_index as i8)? * ANIMATION_BYTES))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            settings: ActorSettings::read(section(4)?)?,
            resources: EnemyResources::read(bytes)?,
            rows,
            commands: serde_json::from_value(action_program::physical_command_table(section(
                14,
            )?)?)?,
            rules: section(8)?
                .chunks_exact(RULE_BYTES)
                .map(HitRuleRecord::read)
                .collect::<Result<_>>()?,
            hits: (0..hit_extent)
                .step_by(HIT_BYTES)
                .map(|offset| Ok((offset / HIT_BYTES, HitRecord::read(&hit_pool[offset..])?)))
                .collect::<Result<_>>()?,
            animations: animation_table::decode(animation_pool, roots)?,
        })
    }
}
