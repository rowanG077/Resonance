//! World contact and scripted story visibility, evaluated from persistent progress.
use super::{Position, World, travel::Mount};
use anyhow::{Result, ensure};
use resonance_content::overworld::{Guidepost, Interaction, Landmark, Landmarks, Marker};
use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::TAU,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub interaction: Interaction,
    pub marker: Marker,
}

pub struct Progress<'a> {
    pub memory: &'a symphonia_script_vm::Memory,
    pub items: &'a BTreeMap<u16, u8>,
    pub script_state: &'a symphonia_script::authored::ScriptState,
    pub visited: &'a BTreeSet<u16>,
    pub event_flags: &'a BTreeSet<u16>,
    pub formation: &'a [u8],
}

impl<'a> Progress<'a> {
    pub(super) fn new(
        memory: &'a symphonia_script_vm::Memory,
        party: &'a resonance_events::party::Party,
        event_flags: &'a BTreeSet<u16>,
        script_state: &'a symphonia_script::authored::ScriptState,
    ) -> Self {
        Self {
            memory,
            items: &party.items,
            script_state,
            visited: &party.travel.visited_locations,
            event_flags,
            formation: &party.formation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contact {
    pub id: u16,
    pub direction: u8,
    pub blocked: bool,
}

pub struct Locations {
    pub(super) definitions: Arc<Landmarks>,
    pub(super) guideposts: Arc<Vec<Guidepost>>,
    pub(super) appearances: BTreeMap<u16, Appearance>,
    pub(super) story_rules: Arc<super::scripts::Rules>,
}
impl Locations {
    pub fn new(
        definitions: Arc<Landmarks>,
        guideposts: Arc<Vec<Guidepost>>,
        story_rules: Arc<super::scripts::Rules>,
    ) -> Result<Self> {
        definitions.validate()?;
        let mut ids = BTreeSet::new();
        for guidepost in guideposts.iter() {
            guidepost.validate(&definitions)?;
            ensure!(ids.insert(guidepost.location), "duplicate world guidepost");
        }
        let appearances = Self::default_appearances(&definitions);
        Ok(Self {
            definitions,
            guideposts,
            appearances,
            story_rules,
        })
    }
    pub(super) fn default_appearances(definitions: &Landmarks) -> BTreeMap<u16, Appearance> {
        definitions
            .worlds
            .iter()
            .flatten()
            .map(|l| {
                (
                    l.id,
                    Appearance {
                        interaction: l.interaction,
                        marker: l.marker,
                    },
                )
            })
            .collect()
    }
    pub fn definition(&self, id: u16) -> Option<&Landmark> {
        self.definitions
            .worlds
            .get(usize::from(id / 256))?
            .get(usize::from((id & 255).checked_sub(1)?))
    }
    pub fn appearance(&self, id: u16) -> Option<Appearance> {
        self.appearances.get(&id).copied()
    }
    pub fn visible(&self, world: World) -> impl Iterator<Item = (&Landmark, Appearance)> {
        self.definitions.worlds[world.index()]
            .iter()
            .filter_map(|l| {
                let state = self.appearances[&l.id];
                (state.marker != Marker::None
                    && (state.marker != Marker::FieldPoint
                        || state.interaction != Interaction::Disabled))
                    .then_some((l, state))
            })
    }
    pub fn reward(&self, id: u16) -> Option<u16> {
        self.definitions.item_rewards.get(&id).copied()
    }
    pub fn guidepost(&self, id: u16) -> Option<&Guidepost> {
        self.guideposts.iter().find(|g| g.location == id)
    }

    /// Place a returning ground traveller outside active circular entrances,
    /// in catalogue order, with ten units of extra clearance.
    pub(super) fn push_out(
        &self,
        world: World,
        mut position: Position,
        mount: Mount,
    ) -> Result<Position> {
        if !matches!(mount, Mount::Foot | Mount::Noishe) {
            return Ok(position);
        }
        for landmark in &self.definitions.worlds[world.index()] {
            if self.appearances[&landmark.id].interaction == Interaction::Disabled
                || (mount == Mount::Noishe && landmark.marker == Marker::FieldPoint)
            {
                continue;
            }
            let center = Position::from_map([
                landmark.position[0],
                landmark.position[1],
                position.map()[2],
            ])?;
            let [dx, dz] = center.displacement_to(position);
            if dx.hypot(dz) <= landmark.radius + 50. {
                let angle = dx.atan2(-dz);
                let radius = landmark.radius + 60.;
                position = center.translated([
                    radius * super::collision::sine(angle),
                    -radius * super::collision::cosine(angle),
                    0.,
                ])?;
            }
        }
        Ok(position)
    }

    /// Evaluate the prepared script against the current progress, publishing
    /// appearances only after the entire refresh succeeds.
    pub fn refresh(&mut self, p: &Progress<'_>) -> Result<()> {
        self.appearances = self.story_rules.refresh(self, p)?;
        Ok(())
    }

    /// Probes the proposed position before movement commits. Airborne contact
    /// is restricted to the towers and the height-sensitive flying dragon.
    pub fn contact(
        &self,
        world: World,
        position: Position,
        mount: Mount,
        altitude: f32,
    ) -> Option<Contact> {
        if mount == Mount::Ship {
            return None;
        }
        self.definitions.worlds[world.index()]
            .iter()
            .find_map(|landmark| {
                let appearance = self.appearances[&landmark.id];
                if appearance.interaction == Interaction::Disabled {
                    return None;
                }
                if mount.airborne() {
                    match landmark.id {
                        24 | 280 => {}
                        304 if (landmark.height.unwrap_or(0.) - altitude).abs() < 200. => {}
                        _ => return None,
                    }
                } else if mount != Mount::Foot && landmark.marker == Marker::FieldPoint {
                    return None;
                }
                let center = Position::from_map([landmark.position[0], landmark.position[1], 0.])
                    .expect("validated landmark");
                let [dx, dz] = position.displacement_to(center);
                if dx.hypot(dz) > 50. + landmark.radius {
                    return None;
                }
                // Measure displacement from the landmark to the player.
                let angle = (-dx).atan2(dz);
                let sector = ((TAU + angle) * 16. / TAU) as u32 & 15;
                let direction = match sector {
                    0 | 15 => 2,
                    1 | 2 => 1,
                    3 | 4 => 0,
                    5 | 6 => 7,
                    7 | 8 => 6,
                    9 | 10 => 5,
                    11 | 12 => 4,
                    _ => 3,
                };
                Some(Contact {
                    id: landmark.id,
                    direction,
                    blocked: appearance.interaction == Interaction::Blocked,
                })
            })
    }
}

#[cfg(test)]
pub(super) mod tests;
