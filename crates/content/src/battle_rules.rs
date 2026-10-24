//! Editable shared battle rules. Source provenance stays with the importer;
//! native table indices, ordering and gameplay constraints remain authoritative.
use crate::{
    battle_defense_system as defense, battle_formation as formation, battle_profile as profile,
    battle_projectile as projectile, battle_recoil as recoil, battle_unison as unison,
    library::Snapshot, preparation::JSON_LIMIT, prepared::Files,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, sync::Arc};

pub const VERSION: u32 = 1;
pub const PARTY: &str = "battle/party/rules.json";
pub const NORMAL_ATTACKS: &str = "battle/party/normal-attacks.json";
pub const MARTIAL_ATTACKS: &str = "battle/party/techniques.json";

type Reader<'a> = dyn FnMut(&str) -> Result<Arc<[u8]>> + 'a;

fn json<T: DeserializeOwned>(read: &mut Reader<'_>, path: &str) -> Result<T> {
    crate::library::validate_path(path)?;
    let bytes = read(path)?;
    ensure!(
        bytes.len() <= JSON_LIMIT,
        "battle rules exceed size limit: {path}"
    );
    serde_json::from_slice(&bytes).with_context(|| format!("battle rules {path}"))
}

fn version(value: u32, path: &str) -> Result<()> {
    ensure!(
        value == VERSION,
        "unsupported battle rules version in {path}"
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Formations {
    pub version: u32,
    pub records: Vec<formation::Formation>,
    pub pools: Vec<Vec<formation::WeightedFormation>>,
}

impl From<formation::Formations> for Formations {
    fn from(value: formation::Formations) -> Self {
        Self {
            version: VERSION,
            records: value.records,
            pools: value.pools,
        }
    }
}

fn read_formations(read: &mut Reader<'_>) -> Result<formation::Formations> {
    let value: Formations = json(read, formation::PATH)?;
    version(value.version, formation::PATH)?;
    let value = formation::Formations {
        source_sha256: String::new(),
        records: value.records,
        pools: value.pools,
    };
    // The encounter boundary validates selected rows. A malformed unused row
    // must not prevent an unrelated, valid formation from being prepared.
    ensure!(
        !value.records.is_empty() && value.records.len() <= usize::from(u16::MAX) + 1,
        "invalid formation catalogue"
    );
    for pool in &value.pools {
        ensure!(
            !pool.is_empty()
                && pool
                    .iter()
                    .all(|entry| entry.weight > 0
                        && usize::from(entry.formation) < value.records.len()),
            "invalid encounter pool"
        );
    }
    Ok(value)
}

/// Large attack tables have separate editing boundaries; ordinary profiles and
/// selection policies stay together. These are direct references, not inventories.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Party {
    pub version: u32,
    pub normal_attacks: String,
    pub martial_attacks: String,
    pub body_colliders: [profile::StandingCollider; 9],
    pub default_strategy: [[u8; 3]; 10],
    pub companion_policy: profile::CompanionPolicy,
    pub entry: profile::Entry,
    pub chant_sequences: BTreeMap<u8, Vec<(u16, crate::battle_action::ActionPose)>>,
    pub voice_sequences: Vec<profile::VoicePolicy>,
    pub death_voice_pairs: Vec<[u8; 2]>,
    pub contact_sounds: profile::ContactSounds,
    pub overlimit_voices: [Option<crate::battle_voice::Sound>; 9],
    pub item_use_offsets: [[f32; 3]; 9],
    pub records: Vec<profile::Profile>,
    pub lethal_rescue_names: [String; 5],
}

impl Party {
    pub fn from_table(
        value: profile::Table,
        normal_attacks: String,
        martial_attacks: String,
    ) -> Self {
        Self {
            version: VERSION,
            normal_attacks,
            martial_attacks,
            body_colliders: value.body_colliders,
            default_strategy: value.default_strategy,
            companion_policy: value.companion_policy,
            entry: value.entry,
            chant_sequences: value.chant_sequences,
            voice_sequences: value.voice_sequences,
            death_voice_pairs: value.death_voice_pairs,
            contact_sounds: value.contact_sounds,
            overlimit_voices: value.overlimit_voices,
            item_use_offsets: value.item_use_offsets,
            records: value.records,
            lethal_rescue_names: value.lethal_rescue_names,
        }
    }
}

fn read_party(read: &mut Reader<'_>) -> Result<profile::Table> {
    let value: Party = json(read, PARTY)?;
    version(value.version, PARTY)?;
    let value = profile::Table {
        version: profile::PARTY_VERSION,
        source_sha256: String::new(),
        collider_recipe_sha256: String::new(),
        normal_attacks: json(read, &value.normal_attacks)?,
        martial_attacks: json(read, &value.martial_attacks)?,
        body_colliders: value.body_colliders,
        default_strategy: value.default_strategy,
        companion_policy: value.companion_policy,
        entry: value.entry,
        chant_sequences: value.chant_sequences,
        voice_sequences: value.voice_sequences,
        death_voice_pairs: value.death_voice_pairs,
        contact_sounds: value.contact_sounds,
        overlimit_voices: value.overlimit_voices,
        item_use_offsets: value.item_use_offsets,
        records: value.records,
        lethal_rescue_names: value.lethal_rescue_names,
    };
    value.validate()?;
    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefenseSystem {
    pub version: u32,
    pub core: defense::FixedMember,
    pub repair: defense::FixedMember,
    pub orbit: u8,
    pub orbit_count: u8,
    pub arm: u8,
    pub arm_slots: [u8; 4],
    pub deployment: defense::Deployment,
    pub repair_cycle: defense::RepairCycle,
    pub repair_sound: u16,
    pub repair_sound_priority: u8,
    pub messages: defense::Messages,
    pub camera_yaw_limits: [f32; 2],
}

impl From<defense::Definition> for DefenseSystem {
    fn from(value: defense::Definition) -> Self {
        Self {
            version: VERSION,
            core: value.core,
            repair: value.repair,
            orbit: value.orbit,
            orbit_count: value.orbit_count,
            arm: value.arm,
            arm_slots: value.arm_slots,
            deployment: value.deployment,
            repair_cycle: value.repair_cycle,
            repair_sound: value.repair_sound,
            repair_sound_priority: value.repair_sound_priority,
            messages: value.messages,
            camera_yaw_limits: value.camera_yaw_limits,
        }
    }
}

fn read_defense(read: &mut Reader<'_>) -> Result<defense::Definition> {
    let value: DefenseSystem = json(read, defense::PATH)?;
    version(value.version, defense::PATH)?;
    let value = defense::Definition {
        version: defense::VERSION,
        source_sha256: String::new(),
        recipe_sha256: String::new(),
        core: value.core,
        repair: value.repair,
        orbit: value.orbit,
        orbit_count: value.orbit_count,
        arm: value.arm,
        arm_slots: value.arm_slots,
        deployment: value.deployment,
        repair_cycle: value.repair_cycle,
        repair_sound: value.repair_sound,
        repair_sound_priority: value.repair_sound_priority,
        messages: value.messages,
        camera_yaw_limits: value.camera_yaw_limits,
    };
    value.validate()?;
    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unison {
    pub version: u32,
    pub host: Host,
    pub openers: Vec<unison::Opener>,
    pub combinations: Vec<unison::Combination>,
    pub placements: Vec<unison::Placement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    pub placement: [[f32; 3]; 4],
    pub minimum_distance: f32,
    pub short_weapon_penalty: f32,
    pub restore_height: f32,
    pub windup_rate: f32,
    pub windup_color: [u8; 3],
    pub prelude: unison::Prelude,
}

impl From<unison::Catalogue> for Unison {
    fn from(value: unison::Catalogue) -> Self {
        let host = value.host;
        Self {
            version: VERSION,
            host: Host {
                placement: host.placement,
                minimum_distance: host.minimum_distance,
                short_weapon_penalty: host.short_weapon_penalty,
                restore_height: host.restore_height,
                windup_rate: host.windup_rate,
                windup_color: host.windup_color,
                prelude: host.prelude,
            },
            openers: value.openers,
            combinations: value.combinations,
            placements: value.placements,
        }
    }
}

fn read_unison(read: &mut Reader<'_>) -> Result<unison::Catalogue> {
    let value: Unison = json(read, unison::PATH)?;
    version(value.version, unison::PATH)?;
    let host = value.host;
    let value = unison::Catalogue {
        source_sha256: String::new(),
        host: unison::Host {
            source_sha256: String::new(),
            placement: host.placement,
            minimum_distance: host.minimum_distance,
            short_weapon_penalty: host.short_weapon_penalty,
            restore_height: host.restore_height,
            windup_rate: host.windup_rate,
            windup_color: host.windup_color,
            prelude: host.prelude,
        },
        openers: value.openers,
        combinations: value.combinations,
        placements: value.placements,
    };
    value.validate()?;
    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projectiles {
    pub version: u32,
    pub records: Vec<projectile::Projectile>,
    pub resident_spells: BTreeMap<u16, projectile::ResidentSpell>,
    pub support_durations: BTreeMap<u16, u16>,
    pub fire_ball: Option<projectile::FireBall>,
}

impl From<projectile::Table> for Projectiles {
    fn from(value: projectile::Table) -> Self {
        Self {
            version: VERSION,
            records: value.records,
            resident_spells: value.resident_spells,
            support_durations: value.support_durations,
            fire_ball: value.fire_ball,
        }
    }
}

fn read_projectiles(read: &mut Reader<'_>) -> Result<projectile::Table> {
    let value: Projectiles = json(read, projectile::PATH)?;
    version(value.version, projectile::PATH)?;
    // Full projectile admission needs the selected actor/rig and remains in
    // encounter preparation. Preserve unused unsupported templates verbatim.
    Ok(projectile::Table {
        source_sha256: String::new(),
        records: value.records,
        resident_spells: value.resident_spells,
        support_durations: value.support_durations,
        fire_ball: value.fire_ball,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recoil {
    pub version: u32,
    pub impulses: Vec<[f32; 2]>,
    pub suppression_distance: f32,
    pub guard_speed: f32,
    pub light_vertical_scale: f32,
    pub heavy_vertical_scale: f32,
    pub default_guard_recovery_bonuses: Vec<u8>,
}

impl From<recoil::Table> for Recoil {
    fn from(value: recoil::Table) -> Self {
        Self {
            version: VERSION,
            impulses: value.impulses,
            suppression_distance: value.suppression_distance,
            guard_speed: value.guard_speed,
            light_vertical_scale: value.light_vertical_scale,
            heavy_vertical_scale: value.heavy_vertical_scale,
            default_guard_recovery_bonuses: value.default_guard_recovery_bonuses,
        }
    }
}

fn read_recoil(read: &mut Reader<'_>) -> Result<recoil::Table> {
    let value: Recoil = json(read, recoil::PATH)?;
    version(value.version, recoil::PATH)?;
    Ok(recoil::Table {
        source_sha256: String::new(),
        impulses: value.impulses,
        suppression_distance: value.suppression_distance,
        guard_speed: value.guard_speed,
        light_vertical_scale: value.light_vertical_scale,
        heavy_vertical_scale: value.heavy_vertical_scale,
        default_guard_recovery_bonuses: value.default_guard_recovery_bonuses,
    })
}

pub struct Captured {
    pub formations: formation::Formations,
    pub party: profile::Table,
    pub defense: defense::Definition,
    pub unison: unison::Catalogue,
    pub projectiles: projectile::Table,
    pub recoil: recoil::Table,
    pub techniques: crate::arte::Catalogue,
}

pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<Captured> {
    let mut read = |path: &str| {
        ensure!(!cancelled(), "battle rules preparation cancelled");
        snapshot.read(path, JSON_LIMIT)
    };
    let result = Captured {
        formations: read_formations(&mut read)?,
        party: read_party(&mut read)?,
        defense: read_defense(&mut read)?,
        unison: read_unison(&mut read)?,
        projectiles: read_projectiles(&mut read)?,
        recoil: read_recoil(&mut read)?,
        techniques: json(&mut read, crate::arte::PATH)?,
    };
    result.techniques.validate()?;
    ensure!(!cancelled(), "battle rules preparation cancelled");
    Ok(result)
}

pub fn formations(files: &Files) -> Result<formation::Formations> {
    read_formations(&mut |path| files.read(path))
}

/// Pool selection and encounter preparation must share the same captured table.
pub fn capture_formations(snapshot: &mut Snapshot) -> Result<formation::Formations> {
    read_formations(&mut |path| snapshot.read(path, JSON_LIMIT))
}
pub fn party(files: &Files) -> Result<profile::Table> {
    read_party(&mut |path| files.read(path))
}
pub fn defense(files: &Files) -> Result<defense::Definition> {
    read_defense(&mut |path| files.read(path))
}
pub fn unison(files: &Files) -> Result<unison::Catalogue> {
    read_unison(&mut |path| files.read(path))
}
pub fn projectiles(files: &Files) -> Result<projectile::Table> {
    read_projectiles(&mut |path| files.read(path))
}
pub fn recoil(files: &Files) -> Result<recoil::Table> {
    read_recoil(&mut |path| files.read(path))
}
