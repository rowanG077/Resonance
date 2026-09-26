//! Resolve an encounter's enemy requirements before combat or resource activation.
mod assets;
pub use assets::{Assets, Inputs};
mod preparation;
mod render;
mod resources;
use anyhow::{Context, Result, ensure};
pub use preparation::PrepareOptions;
pub use render::{RenderEffect, RenderModel, RenderTrail};
use resonance_content::{
    battle_formation::{Formations, PATH},
    monster::{Monster, MonsterBook},
    prepared::Files,
};
use std::sync::Arc;

/// A complete, inactive candidate. Presentation prepares these descriptors before
/// constructing the live battle; every ID belongs only to this candidate.
pub struct Prepared {
    pub core: Arc<resonance_battle::PreparedBattle>,
    pub entry_transition: super::entry_transition::EntryTransition,
    pub lifecycle: super::lifecycle::PreparedLifecycle,
    pub models: Vec<RenderModel>,
    pub effects: Vec<RenderEffect>,
    pub trails: Vec<RenderTrail>,
    pub characters: Vec<u8>,
    pub actors: Vec<ActorActions>,
    pub music: u16,
    pub results: super::results::Setup,
}

/// Prepared action handles and original policy inputs. Decisions operate on the
/// core's actors; this table never stores a second copy of combat state.
pub struct ActorActions {
    pub actor: resonance_battle::ActorId,
    pub target: resonance_battle::ActorId,
    pub normals: Option<[u16; 7]>,
    pub techniques: std::collections::BTreeMap<u16, u16>,
    pub enemy_actions: Vec<u16>,
    pub fidget_ticks: u16,
    pub strategy: [u8; 3],
}

/// Original 1B5A4 groups the active formation in strategy rows. Within each
/// row it preserves formation order; only the leader's row uses fixed Z slots.
pub fn party_positions(
    files: &Files,
    menus: &resonance_content::menu_data::MenuData,
    party: &resonance_events::party::Party,
) -> Result<Vec<[f32; 3]>> {
    let profiles: resonance_content::battle_profile::Table =
        files.json(resonance_content::battle_profile::PARTY_PATH)?;
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
            ensure!(member.strategy[2] < 7, "invalid battle position strategy");
            Ok(menus.strategy.lane(index, member.strategy[2]))
        })
        .collect::<Result<Vec<_>>>()?;
    place_party(&profiles.placement, &lanes)
}

fn place_party(
    placement: &resonance_content::battle_profile::Placement,
    lanes: &[usize],
) -> Result<Vec<[f32; 3]>> {
    ensure!(
        !lanes.is_empty() && lanes.len() <= 4 && lanes.iter().all(|&lane| lane < 3),
        "invalid party placement rows"
    );
    ensure!(
        placement
            .leader_z
            .iter()
            .chain(&placement.single_row_z)
            .chain([
                &placement.front_x,
                &placement.row_step,
                &placement.member_x,
                &placement.member_z,
                &placement.other_row_center,
            ])
            .all(|value| value.is_finite()),
        "invalid party placement operands"
    );
    let mut positions = vec![[0.; 3]; lanes.len()];
    for row in 0..3 {
        let members: Vec<_> = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, &lane)| (lane == row).then_some(index))
            .collect();
        let row_x = placement.row_step.mul_add(row as f32, placement.front_x);
        for (order, &index) in members.iter().enumerate() {
            let z = if row == lanes[0] {
                placement.leader_z[order]
            } else {
                let mut z = placement.other_row_center.mul_add(
                    (members.len() >> 1) as f32,
                    placement.member_z * order as f32,
                );
                if members.len() == 1 {
                    z += placement.single_row_z[row & 1];
                }
                z
            };
            positions[index] = [placement.member_x.mul_add(order as f32, row_x), 0., z];
        }
    }
    Ok(positions)
}

pub struct Enemies {
    /// Keep every declared resource, including reserves with no initial actor.
    pub resources: Vec<Arc<Monster>>,
    pub spawns: Vec<Spawn>,
    pub escape_restricted: bool,
    pub flags: u8,
    /// Original formation mask, indexed by declared enemy resource group.
    pub hidden_names: u8,
}

pub struct Spawn {
    pub resource: usize,
    pub variant: usize,
    pub appearance: u8,
    pub attachments: [u8; 2],
    /// X/Z supplied by the encounter, or None for original automatic placement.
    pub position: Option<[i16; 2]>,
}

/// The catalogue is the session's already loaded Monster Book. Both consumers
/// use its shared statistics and model publications. This resolves requirements;
/// it does not certify that models, behavior or battle presentation are ready.
pub fn enemies(files: &Files, catalogue: &MonsterBook, formation: u16) -> Result<Enemies> {
    let formations: Formations = files.json(PATH)?;
    formations.validate()?;
    let row = formations
        .records
        .get(usize::from(formation))
        .with_context(|| format!("unknown battle formation {formation}"))?;
    let resources = row.resources[..usize::from(row.resource_count)]
        .iter()
        .map(|&id| {
            let monster = usize::try_from(id)
                .ok()
                .and_then(|id| catalogue.records.get(id))
                .with_context(|| format!("formation {formation} requires missing enemy {id}"))?;
            ensure!(i16::from(monster.id) == id, "unordered enemy catalogue");
            monster.validate(528)?;
            Ok(Arc::new(monster.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    let spawns = row.actors[..usize::from(row.actor_count)]
        .iter()
        .map(|actor| {
            let resource = usize::from(actor.resource);
            let variant = usize::from(actor.variant);
            ensure!(
                variant < resources[resource].statistics.len(),
                "formation {formation} requires missing enemy {} variant {variant}",
                resources[resource].id
            );
            Ok(Spawn {
                resource,
                variant,
                appearance: actor.appearance,
                attachments: actor.attachments,
                position: (row.flags & 2 == 0).then_some(actor.position),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Enemies {
        resources,
        spawns,
        escape_restricted: row.flags & 1 != 0,
        flags: row.flags,
        hidden_names: row.hidden_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        battle_formation::{Actor, Formation},
        monster::MonsterStats,
    };

    #[test]
    fn party_rows_keep_formation_order_and_follow_the_actual_leader() -> Result<()> {
        let placement = resonance_content::battle_profile::Placement {
            leader_z: [0., -250., 250., -450.],
            front_x: -300.,
            row_step: -200.,
            member_x: -50.,
            member_z: -400.,
            other_row_center: 200.,
            single_row_z: [150., -150.],
        };
        assert_eq!(
            place_party(&placement, &[0, 1, 2])?,
            [[-300., 0., 0.], [-500., 0., -150.], [-700., 0., 150.],]
        );
        assert_eq!(
            place_party(&placement, &[2, 0, 2, 0])?,
            [
                [-700., 0., 0.],
                [-300., 0., 200.],
                [-750., 0., -250.],
                [-350., 0., -200.],
            ]
        );
        assert!(place_party(&placement, &[]).is_err());
        assert!(place_party(&placement, &[3]).is_err());
        assert!(place_party(&placement, &[0; 5]).is_err());
        assert!(
            place_party(
                &resonance_content::battle_profile::Placement {
                    member_z: f32::NAN,
                    ..placement
                },
                &[0]
            )
            .is_err()
        );
        Ok(())
    }

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
        let mut actors = [Actor::default(); 8];
        actors[0] = Actor {
            resource: 0,
            variant: 1,
            appearance: 4,
            attachments: [2, 3],
            position: [-50, 30],
        };
        let formations = Formations {
            source_sha256: "a".repeat(64),
            records: vec![Formation {
                actor_count: 2,
                resource_count: 2,
                flags: 1,
                hidden_names: 3,
                resources: [0, 1, -1, -1],
                actors,
                storage: [0; 8],
            }],
        };
        let mut files = Files::default();
        files.bytes.insert(
            PATH.into(),
            Arc::from(serde_json::to_vec(&formations).unwrap()),
        );
        let reserve = Monster {
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
        files.bytes.insert(
            PATH.into(),
            Arc::from(serde_json::to_vec(&formations).unwrap()),
        );
    }

    #[test]
    fn selects_variants_and_attachments_and_retains_reserve_resources() {
        let (files, catalogue) = inputs();
        let selected = enemies(&files, &catalogue, 0).unwrap();
        assert_eq!(selected.resources.len(), 2);
        assert_eq!(selected.spawns.len(), 2);
        assert!(selected.escape_restricted);
        assert_eq!(selected.hidden_names, 3);
        let first = &selected.spawns[0];
        let second = &selected.spawns[1];
        assert_eq!(first.resource, second.resource);
        assert_eq!(
            selected.resources[first.resource].statistics[first.variant].hp,
            700
        );
        assert_eq!(
            selected.resources[second.resource].statistics[second.variant].hp,
            320
        );
        assert_eq!(first.appearance, 4);
        assert_eq!(first.attachments, [2, 3]);
        assert_eq!(first.position, Some([-50, 30]));
        assert_eq!(selected.resources[1].id, 1);
    }

    #[test]
    fn automatic_placement_does_not_use_stored_coordinates() {
        let (mut files, catalogue) = inputs();
        edit(&mut files, |row| row.flags = 2);
        let selected = enemies(&files, &catalogue, 0).unwrap();
        assert!(!selected.escape_restricted);
        assert!(selected.spawns.iter().all(|spawn| spawn.position.is_none()));
    }

    #[test]
    fn missing_inputs_resources_variants_and_invalid_slots_fail_before_activation() {
        let (files, catalogue) = inputs();
        assert!(enemies(&Files::default(), &catalogue, 0).is_err());
        assert!(enemies(&files, &catalogue, 1).is_err());
        for change in [
            |row: &mut Formation| row.resources[1] = -1,
            |row: &mut Formation| row.resources[1] = 2,
            |row: &mut Formation| row.actors[0].variant = 2,
            |row: &mut Formation| row.actors[0].resource = 2,
        ] {
            let mut changed = files.clone();
            edit(&mut changed, change);
            assert!(enemies(&changed, &catalogue, 0).is_err());
        }
        assert_eq!(catalogue.records[0].statistics[0].hp, 320);
    }
}
