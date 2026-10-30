//! Named regions and terrain rules; dense lookup tables exist only in memory.
use super::*;
use anyhow::{Context, bail};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Groups {
    Both(u16),
    Symbols([u16; 2]),
}
impl Groups {
    fn pair(&self) -> [u16; 2] {
        match self {
            Self::Both(group) => [*group; 2],
            Self::Symbols(pair) => *pair,
        }
    }
    pub fn new(pair: [u16; 2]) -> Self {
        if pair[0] == pair[1] {
            Self::Both(pair[0])
        } else {
            Self::Symbols(pair)
        }
    }
}

pub type TerrainGroups = BTreeMap<Terrain, Groups>;
pub type SurfaceTerrain = BTreeMap<TerrainSurface, Option<Terrain>>;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum RegionGroups {
    Shared(String),
    Inline(TerrainGroups),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: u8,
    pub groups: RegionGroups,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub later: Option<RegionGroups>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continent {
    pub arenas: BTreeMap<Terrain, u8>,
    pub grid: Vec<[u8; AREA_COLUMNS]>,
    pub regions: BTreeMap<String, Region>,
    /// Overrides only the listed surfaces; null disables encounters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub surface_terrain: SurfaceTerrain,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub position: [i32; 2],
    #[serde(default, skip_serializing_if = "zero")]
    pub height: f32,
}
fn zero(value: &f32) -> bool {
    *value == 0.
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dock {
    pub land: Endpoint,
    pub sea: Endpoint,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub default_group: Groups,
    pub surface_terrain: SurfaceTerrain,
    /// Ordered: overlapping docks select the last match; embark falls back to the first.
    pub docks: Vec<Dock>,
    pub later_story: i32,
    pub sylvarant: Continent,
    pub tethe_alla: Continent,
}

fn surfaces(values: &[Option<Terrain>; SURFACES]) -> SurfaceTerrain {
    values
        .iter()
        .enumerate()
        .filter_map(|(id, &terrain)| terrain.map(|_| (TerrainSurface::ALL[id], terrain)))
        .collect()
}

impl Serialize for EncounterTables {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut counts = BTreeMap::<[u16; 2], usize>::new();
        for pair in self
            .worlds
            .iter()
            .flat_map(|w| w.areas.iter().flatten())
            .flat_map(|a| a.encounter_groups)
        {
            *counts.entry(pair).or_default() += 1;
        }
        let default = counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(pair, _)| pair)
            .unwrap_or([0; 2]);
        let continent = |index: usize| -> Result<Continent> {
            let world = &self.worlds[index];
            let mut regions = BTreeMap::new();
            let mut owners = BTreeMap::<[[u16; 2]; TERRAINS], String>::new();
            let mut later_owners = BTreeMap::<[[u16; 2]; TERRAINS], String>::new();
            let compact = |groups: [[u16; 2]; TERRAINS]| -> TerrainGroups {
                Terrain::ALL
                    .into_iter()
                    .zip(groups)
                    .filter_map(|(terrain, pair)| {
                        (pair != default).then(|| (terrain, Groups::new(pair)))
                    })
                    .collect()
            };
            for (id, area) in world.areas.iter().enumerate() {
                let Some(area) = area else { continue };
                let name = world.area_names[id]
                    .as_ref()
                    .context("encounter region has no name")?
                    .clone();
                ensure!(!name.is_empty(), "empty encounter region name");
                let groups = if let Some(owner) = owners.get(&area.encounter_groups) {
                    RegionGroups::Shared(owner.clone())
                } else {
                    owners.insert(area.encounter_groups, name.clone());
                    RegionGroups::Inline(compact(area.encounter_groups))
                };
                let later = if index == 1 {
                    let late = self.worlds[2]
                        .areas
                        .get(id)
                        .and_then(Option::as_ref)
                        .context("missing later encounter region")?;
                    if late.encounter_groups == area.encounter_groups {
                        if matches!(groups, RegionGroups::Inline(_)) {
                            later_owners
                                .entry(late.encounter_groups)
                                .or_insert_with(|| name.clone());
                        }
                        None
                    } else if let Some(owner) = later_owners.get(&late.encounter_groups) {
                        Some(RegionGroups::Shared(owner.clone()))
                    } else {
                        later_owners.insert(late.encounter_groups, name.clone());
                        Some(RegionGroups::Inline(compact(late.encounter_groups)))
                    }
                } else {
                    None
                };
                ensure!(
                    regions
                        .insert(
                            name,
                            Region {
                                id: id as u8,
                                groups,
                                later
                            }
                        )
                        .is_none(),
                    "duplicate encounter region name"
                );
            }
            Ok(Continent {
                arenas: Terrain::ALL
                    .into_iter()
                    .zip(world.arena_by_terrain)
                    .collect(),
                grid: self.area_grids[index].clone(),
                regions,
                surface_terrain: self.surface_terrain[index]
                    .iter()
                    .enumerate()
                    .filter_map(|(id, &terrain)| {
                        (terrain != self.surface_terrain[0][id])
                            .then_some((TerrainSurface::ALL[id], terrain))
                    })
                    .collect(),
            })
        };
        let endpoint = |p: &TravelEndpoint| Endpoint {
            position: [p.map_x, p.map_z],
            height: p.height,
        };
        Definition {
            default_group: Groups::new(default),
            surface_terrain: surfaces(&self.surface_terrain[0]),
            docks: self
                .travel_points
                .iter()
                .map(|p| Dock {
                    land: endpoint(&p[0]),
                    sea: endpoint(&p[1]),
                })
                .collect(),
            later_story: self.later_story,
            sylvarant: continent(0).map_err(serde::ser::Error::custom)?,
            tethe_alla: continent(1).map_err(serde::ser::Error::custom)?,
        }
        .serialize(serializer)
    }
}

impl Definition {
    fn prepare(self) -> Result<EncounterTables> {
        fn apply_surfaces(
            result: &mut [Option<Terrain>; SURFACES],
            source: &SurfaceTerrain,
        ) -> Result<()> {
            for (&id, &terrain) in source {
                *result
                    .get_mut(id.index())
                    .context("invalid terrain surface")? = terrain;
            }
            Ok(())
        }
        let mut base_surface = [None; SURFACES];
        apply_surfaces(&mut base_surface, &self.surface_terrain)?;
        let mut worlds = Vec::new();
        let mut surface_terrain = Vec::new();
        let mut area_grids = Vec::new();
        let default = self.default_group.pair();
        let make_world = |source: &Continent,
                          kind: WorldKind,
                          later: bool|
         -> Result<EncounterWorld> {
            ensure!(
                source.arenas.len() == TERRAINS,
                "world must name all ten terrain arenas"
            );
            let mut arena_by_terrain = [0; TERRAINS];
            for (terrain, arena) in &source.arenas {
                arena_by_terrain[terrain.index()] = *arena;
            }
            let size = source
                .regions
                .values()
                .map(|r| usize::from(r.id) + 1)
                .max()
                .context("world has no encounter regions")?;
            let mut areas = vec![None; size];
            let mut area_names = vec![None; size];
            let mut ids = BTreeSet::new();
            for (name, region) in &source.regions {
                ensure!(
                    !name.is_empty() && ids.insert(region.id),
                    "empty region name or duplicate region ID"
                );
                let later_override = later && region.later.is_some();
                let selected = region
                    .later
                    .as_ref()
                    .filter(|_| later)
                    .unwrap_or(&region.groups);
                let groups = match selected {
                    RegionGroups::Inline(groups) => groups,
                    RegionGroups::Shared(owner) => {
                        ensure!(owner != name, "region {name} refers to itself");
                        let target = source.regions.get(owner).with_context(|| {
                            format!("region {name} refers to missing region {owner}")
                        })?;
                        match target
                            .later
                            .as_ref()
                            .filter(|_| later_override)
                            .unwrap_or(&target.groups)
                        {
                            RegionGroups::Inline(groups) => groups,
                            RegionGroups::Shared(_) => bail!(
                                "region {name} must refer directly to a region with inline groups"
                            ),
                        }
                    }
                };
                let mut encounter_groups = [default; TERRAINS];
                for (terrain, pair) in groups {
                    encounter_groups[terrain.index()] = pair.pair();
                }
                areas[usize::from(region.id)] = Some(EncounterArea { encounter_groups });
                area_names[usize::from(region.id)] = Some(name.clone());
            }
            Ok(EncounterWorld {
                kind,
                area_names,
                areas,
                arena_by_terrain,
            })
        };
        for (source, kind) in [
            (&self.sylvarant, WorldKind::Sylvarant),
            (&self.tethe_alla, WorldKind::TetheAlla),
        ] {
            if kind == WorldKind::Sylvarant {
                ensure!(
                    source.regions.values().all(|r| r.later.is_none()),
                    "later encounters apply only to Tethe'alla"
                );
            }
            worlds.push(make_world(source, kind, false)?);
            let mut surface = base_surface;
            apply_surfaces(&mut surface, &source.surface_terrain)?;
            surface_terrain.push(surface);
            area_grids.push(source.grid.clone());
        }
        worlds.push(make_world(
            &self.tethe_alla,
            WorldKind::TetheAllaLate,
            true,
        )?);
        let endpoint = |p: Endpoint| TravelEndpoint {
            map_x: p.position[0],
            map_z: p.position[1],
            height: p.height,
        };
        let result = EncounterTables {
            worlds,
            surface_terrain,
            area_grids,
            travel_points: self
                .docks
                .into_iter()
                .map(|d| [endpoint(d.land), endpoint(d.sea)])
                .collect(),
            later_story: self.later_story,
        };
        result.validate()?;
        Ok(result)
    }
}
impl<'de> Deserialize<'de> for EncounterTables {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Definition::deserialize(deserializer)?
            .prepare()
            .map_err(serde::de::Error::custom)
    }
}
