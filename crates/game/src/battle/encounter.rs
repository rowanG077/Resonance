//! Resolve an encounter's enemy requirements before combat or resource activation.
mod assets;
pub use assets::{Assets, Inputs};
mod placement;
mod preparation;
mod render;
pub(crate) mod resources;
use anyhow::{Context, Result, ensure};
pub use preparation::{PrepareOptions, PreparedEnemyMember, PreparedPartyMember};
pub use render::{RenderEffect, RenderModel, RenderTrail};
use resonance_content::{
    battle_formation::{Formations, PATH, Settings},
    monster::{Monster, MonsterBook},
    prepared::Files,
};
use std::sync::Arc;

/// A complete, inactive candidate. Presentation prepares these descriptors before
/// constructing the live battle; every ID belongs only to this candidate.
pub struct Prepared {
    pub core: resonance_battle::Battle,
    pub model_player: resonance_battle::Models,
    pub lifecycle: super::lifecycle::Lifecycle,
    pub models: Vec<RenderModel>,
    pub effects: Vec<RenderEffect>,
    pub effect_banks: Vec<resonance_battle::EffectBank>,
    pub feedback: super::feedback::Feedback,
    pub poison_effect: resonance_battle::EffectAppearance,
    pub trails: Vec<RenderTrail>,
    pub characters: Vec<u8>,
    pub music: u16,
    pub results: super::results::Setup,
}

pub struct Enemies {
    /// Keep every declared resource, including reserves with no initial actor.
    pub resources: Vec<EnemyResource>,
    pub spawns: Vec<Spawn>,
    pub settings: Settings,
}

pub struct EnemyResource {
    pub monster: Arc<Monster>,
    pub hidden_name: bool,
}

pub struct Spawn {
    pub resource: usize,
    pub variant: usize,
    /// X/Z supplied by the encounter, or None for automatic placement.
    pub position: Option<[i16; 2]>,
    pub unsupported_reason: Option<String>,
}

/// The catalogue is the session's already loaded Monster Book. Both consumers
/// use its shared statistics and model publications. This resolves requirements;
/// it does not certify that models, behavior or battle presentation are ready.
pub fn enemies(
    files: &Files,
    catalogue: &MonsterBook,
    item_count: usize,
    formation: u16,
) -> Result<Enemies> {
    let formations: Formations = files.json(PATH)?;
    let row = formations
        .records
        .get(usize::from(formation))
        .with_context(|| format!("unknown battle formation {formation}"))?;
    row.validate()?;
    let resources =
        row.resources
            .iter()
            .map(|resource| {
                let id = resource.enemy;
                let monster = catalogue.records.get(usize::from(id)).with_context(|| {
                    format!("formation {formation} requires missing enemy {id}")
                })?;
                ensure!(u16::from(monster.id) == id, "unordered enemy catalogue");
                monster.validate(item_count)?;
                Ok(EnemyResource {
                    monster: Arc::new(monster.clone()),
                    hidden_name: resource.hidden_name,
                })
            })
            .collect::<Result<Vec<_>>>()?;
    let spawns = row
        .actors
        .iter()
        .map(|actor| {
            let resource = usize::from(actor.resource);
            let variant = usize::from(actor.variant);
            ensure!(
                variant < resources[resource].monster.statistics.len(),
                "formation {formation} requires missing enemy {} variant {variant}",
                resources[resource].monster.id
            );
            Ok(Spawn {
                resource,
                variant,
                position: actor.position,
                unsupported_reason: actor.unsupported_reason.clone(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Enemies {
        resources,
        spawns,
        settings: row.settings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        battle_formation::{Actor, Formation, Resource},
        monster::MonsterStats,
    };

    fn inputs() -> (Files, MonsterBook) {
        let statistics = MonsterStats {
            hp: 320,
            tp: 10,
            initial_hp: 0,
            initial_tp: 0,
            attack: 144,
            thrust: 143,
            defense: 10,
            intelligence: 31,
            accuracy: 73,
            evasion: 48,
            luck: 25,
            level: 4,
            experience: 5,
            gald: 20,
        };
        let preview = serde_json::from_value(serde_json::json!({
            "scale":1.,"elevation":0.,"hidden_geometry":[],"behavior":null,"parts":[{
                "scene":{"resource":0,"mesh":"meshes/enemy.glb","textures":[],"materials":[],
                    "translation":[0.,0.,0.],"clips":[],"autoplay":false,"texture_animations":[],"bone_names":[]},
                "attached_to":null,"additive":false}]
        })).unwrap();
        let monster = Monster {
            unseen_count_group: 0,
            version: resonance_content::monster::MONSTER_VERSION,
            id: 0,
            name: "enemy".into(),
            location: "field".into(),
            category: "beast".into(),
            statistics: vec![
                statistics.clone(),
                MonsterStats {
                    hp: 700,
                    ..statistics
                },
            ],
            drops: [None; 2],
            drop_chances: [0; 2],
            grade: 0,
            steal: None,
            attack_element: None,
            affinities: [0; 9],
            weaknesses: vec![],
            resistances: vec![],
            preview,
        };
        let mut actors = vec![Actor::default(); 2];
        actors[0] = Actor {
            resource: 0,
            variant: 1,
            position: Some([-50, 30]),
            unsupported_reason: Some("encounter appearance override is not prepared".into()),
        };
        let formations = Formations {
            source_sha256: "a".repeat(64),
            records: vec![Formation {
                settings: Settings {
                    play_music: true,
                    celebrate: true,
                    entry_voice: true,
                    escape_restricted: true,
                },
                resources: vec![
                    Resource {
                        enemy: 0,
                        hidden_name: true,
                    },
                    Resource {
                        enemy: 1,
                        hidden_name: true,
                    },
                ],
                actors,
            }],
        };
        let mut files = Files::default();
        files.insert(
            PATH.into(),
            Arc::from(serde_json::to_vec(&formations).unwrap()),
        );
        let reserve = Monster {
            unseen_count_group: 0,
            id: 1,
            ..monster.clone()
        };
        (
            files,
            MonsterBook {
                records: vec![monster, reserve],
                labels: Default::default(),
            },
        )
    }

    fn edit(files: &mut Files, change: impl FnOnce(&mut Formation)) {
        let mut formations: Formations = files.json(PATH).unwrap();
        change(&mut formations.records[0]);
        files.insert(
            PATH.into(),
            Arc::from(serde_json::to_vec(&formations).unwrap()),
        );
    }

    #[test]
    fn selects_variants_and_retains_resource_names_reserves_and_spawn_support() {
        let (files, catalogue) = inputs();
        let selected = enemies(&files, &catalogue, 1, 0).unwrap();
        assert_eq!(selected.resources.len(), 2);
        assert_eq!(selected.spawns.len(), 2);
        assert!(selected.settings.escape_restricted);
        assert!(
            selected
                .resources
                .iter()
                .all(|resource| resource.hidden_name)
        );
        let first = &selected.spawns[0];
        let second = &selected.spawns[1];
        assert_eq!(first.resource, second.resource);
        assert_eq!(
            selected.resources[first.resource].monster.statistics[first.variant].hp,
            700
        );
        assert_eq!(
            selected.resources[second.resource].monster.statistics[second.variant].hp,
            320
        );
        assert!(first.unsupported_reason.is_some());
        assert!(second.unsupported_reason.is_none());
        assert_eq!(first.position, Some([-50, 30]));
        assert_eq!(selected.resources[1].monster.id, 1);
    }

    #[test]
    fn automatic_placement_does_not_use_stored_coordinates() {
        let (mut files, catalogue) = inputs();
        edit(&mut files, |row| {
            row.settings.escape_restricted = false;
            for actor in &mut row.actors {
                actor.position = None;
            }
        });
        let selected = enemies(&files, &catalogue, 1, 0).unwrap();
        assert!(!selected.settings.escape_restricted);
        assert!(selected.spawns.iter().all(|spawn| spawn.position.is_none()));
    }

    #[test]
    fn runtime_validates_only_the_selected_formation() -> Result<()> {
        let (mut files, catalogue) = inputs();
        let mut formations: Formations = files.json(PATH)?;
        let mut broken = formations.records[0].clone();
        broken.actors[0].resource = u8::MAX;
        formations.records.push(broken);
        assert!(formations.validate().is_err());
        files.insert(PATH.into(), serde_json::to_vec(&formations)?.into());
        assert_eq!(enemies(&files, &catalogue, 1, 0)?.spawns.len(), 2);
        assert!(enemies(&files, &catalogue, 1, 1).is_err());
        Ok(())
    }

    #[test]
    fn missing_inputs_resources_variants_and_invalid_slots_fail_before_activation() {
        let (files, mut catalogue) = inputs();
        assert!(enemies(&Files::default(), &catalogue, 1, 0).is_err());
        assert!(enemies(&files, &catalogue, 1, 1).is_err());
        for change in [
            |row: &mut Formation| row.resources[1].enemy = 2,
            |row: &mut Formation| row.actors[0].variant = 2,
            |row: &mut Formation| row.actors[0].resource = 2,
        ] {
            let mut changed = files.clone();
            edit(&mut changed, change);
            assert!(enemies(&changed, &catalogue, 1, 0).is_err());
        }
        assert_eq!(catalogue.records[0].statistics[0].hp, 320);
        for id in [7, 700] {
            catalogue.records[0].drops[0] = Some(id);
            catalogue.records[1].steal = Some(id);
            assert!(enemies(&files, &catalogue, usize::from(id) + 1, 0).is_ok());
            assert!(enemies(&files, &catalogue, usize::from(id), 0).is_err());
        }
    }
}
