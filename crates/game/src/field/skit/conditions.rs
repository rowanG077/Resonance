use anyhow::Result;
use resonance_content::{
    overworld::Terrain,
    skit::{SkitCondition, SkitValue},
};
use resonance_events::{EventRuntime, party::Party};

pub(super) struct Context<'a> {
    pub events: &'a EventRuntime,
    pub party: &'a Party,
    pub map: u32,
    pub exploration_ticks: u32,
    pub overworld: Option<(u8, Option<Terrain>)>,
}

impl Context<'_> {
    pub fn matches(&self, condition: &SkitCondition) -> Result<bool> {
        use SkitCondition::*;
        Ok(match condition {
            None => true,
            Never => false,
            Maps([start, end]) => (u32::from(*start)..=u32::from(*end)).contains(&self.map),
            All(conditions) | Any(conditions) => {
                let all = matches!(condition, All(_));
                for condition in conditions {
                    if self.matches(condition)? != all {
                        return Ok(!all);
                    }
                }
                all
            }
            Not(condition) => !self.matches(condition)?,
            Flag(id) => self.events.world.event_flags.contains(id),
            Viewed(id) => self.party.viewed_skits.contains(id),
            Member(id) => self.party.formation.contains(id),
            Item(id) => self.party.items.get(id).is_some_and(|count| *count > 0),
            Terrain(required) => self
                .overworld
                .is_some_and(|(_, terrain)| terrain == Some(*required)),
            Range { value, min, max } => self
                .value(*value)?
                .is_some_and(|v| min.is_none_or(|min| v >= min) && max.is_none_or(|max| v <= max)),
        })
    }

    fn value(&self, value: SkitValue) -> Result<Option<i64>> {
        use SkitValue::*;
        let member = |id: u8| self.party.members.get(usize::from(id.saturating_sub(1)));
        Ok(match value {
            Global(index) => Some(i64::from(resonance_events::script_global(
                self.events.memory(),
                i32::from(index),
            )?)),
            Gald => Some(i64::from(self.party.gald)),
            AffinityRank(id) => member(id).map(|candidate| {
                // Rank every companion, including those currently outside the formation.
                // Ties favor the lower character ID; Lloyd does not participate.
                1 + self
                    .party
                    .members
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(index, other)| {
                        other.affinity > candidate.affinity
                            || (other.affinity == candidate.affinity
                                && *index + 1 < usize::from(id))
                    })
                    .count() as i64
            }),
            Level(id) => member(id).map(|member| i64::from(member.level)),
            BattleParticipation(id) => self
                .party
                .battles
                .participation
                .get(usize::from(id.saturating_sub(1)))
                .copied()
                .map(i64::from),
            Battles => Some(i64::from(self.party.battles.total)),
            HeadgearCategory(id) => member(id).and_then(|member| {
                self.events
                    .resources()
                    .menu_data
                    .as_ref()?
                    .items
                    .get(usize::from(member.equipment[2]))
                    .map(|item| i64::from(item.category))
            }),
            Title(id) => member(id).map(|member| i64::from(member.title)),
            RingMode => {
                let [mode, _]: [u8; 2] = self.party.travel.sorcerers_ring.into();
                Some(i64::from(mode))
            }
            ExplorationSeconds => Some(i64::from(self.exploration_ticks / 60)),
            PlayedSeconds => {
                Some((self.events.world.played_ticks / 60).min(i64::MAX as u64) as i64)
            }
            WorldArea => self.overworld.map(|(area, _)| i64::from(area)),
        })
    }
}
