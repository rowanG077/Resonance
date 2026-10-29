//! Editable field ground, lighting regions and actor collision meshes.
use super::{CollisionGroup, ModelCollision, document::Collision};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

const IGNORE_CHARACTERS: u32 = 1 << 19;
const IGNORE_ENEMIES: u32 = 1 << 20;
const IGNORE_BLOCKS: u32 = 1 << 21;
const NO_BLOCK_SUPPORT: u32 = 1 << 22;
const FLAGS: u32 =
    IGNORE_CHARACTERS | IGNORE_ENEMIES | IGNORE_BLOCKS | NO_BLOCK_SUPPORT | super::SLIDING_SURFACE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Role {
    Ground,
    Lighting,
    Floor,
    Solid,
}

#[derive(Serialize, Deserialize)]
struct Properties {
    role: Role,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    attributes: u32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    light: u32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    ignore_characters: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    ignore_enemies: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    ignore_blocks: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    no_block_support: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    sliding: bool,
}

impl Properties {
    fn from_surface(role: Role, surface: u32) -> Self {
        let lighting = role == Role::Lighting;
        Self {
            role,
            attributes: surface & !if lighting { 31 } else { FLAGS },
            light: if lighting { surface & 31 } else { 0 },
            ignore_characters: !lighting && surface & IGNORE_CHARACTERS != 0,
            ignore_enemies: !lighting && surface & IGNORE_ENEMIES != 0,
            ignore_blocks: !lighting && surface & IGNORE_BLOCKS != 0,
            no_block_support: !lighting && surface & NO_BLOCK_SUPPORT != 0,
            sliding: !lighting && surface & super::SLIDING_SURFACE != 0,
        }
    }

    fn surface(&self) -> Result<u32> {
        let flags = u32::from(self.ignore_characters) * IGNORE_CHARACTERS
            | u32::from(self.ignore_enemies) * IGNORE_ENEMIES
            | u32::from(self.ignore_blocks) * IGNORE_BLOCKS
            | u32::from(self.no_block_support) * NO_BLOCK_SUPPORT
            | u32::from(self.sliding) * super::SLIDING_SURFACE;
        if self.role == Role::Lighting {
            ensure!(
                self.light < 32 && flags == 0,
                "lighting mesh needs a light from 0 to 31, without collision flags"
            );
            ensure!(
                self.attributes & 31 == 0,
                "use light instead of the low five attribute bits"
            );
        } else {
            ensure!(
                self.light == 0,
                "only lighting meshes select a character light"
            );
            ensure!(
                self.attributes & FLAGS == 0,
                "use named collision flags instead of their attribute bits"
            );
        }
        Ok(self.attributes | self.light | flags)
    }
}

fn encode_roles(
    first: (Role, &[CollisionGroup]),
    second: (Role, &[CollisionGroup]),
) -> Result<Vec<u8>> {
    let mut groups = Vec::new();
    for (role, entries) in [first, second] {
        let name = serde_json::to_value(role)?.as_str().unwrap().to_owned();
        for (index, group) in entries.iter().enumerate() {
            if group.triangles.is_empty() {
                continue;
            }
            groups.push((
                group,
                format!("{name}-{}", index + 1),
                serde_json::to_value(Properties::from_surface(role, group.surface))?,
            ));
        }
    }
    crate::collision_mesh::encode(groups)
}

fn decode_roles(
    bytes: &[u8],
    first: Role,
    second: Role,
) -> Result<(Vec<CollisionGroup>, Vec<CollisionGroup>)> {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for (properties, mut group) in crate::collision_mesh::decode(bytes, "role")? {
        let properties: Properties = serde_json::from_value(properties)?;
        group.surface = properties.surface()?;
        if properties.role == first {
            a.push(group);
        } else {
            ensure!(
                properties.role == second,
                "unexpected collision mesh role {:?}",
                properties.role
            );
            b.push(group);
        }
    }
    Ok((a, b))
}

pub fn encode(collision: &Collision) -> Result<Vec<u8>> {
    encode_roles(
        (Role::Ground, &collision.ground),
        (Role::Lighting, &collision.regions),
    )
}

pub fn decode(bytes: &[u8]) -> Result<Collision> {
    let (ground, regions) = decode_roles(bytes, Role::Ground, Role::Lighting)?;
    Ok(Collision { ground, regions })
}

pub fn encode_model(collision: &ModelCollision) -> Result<Vec<u8>> {
    encode_roles(
        (Role::Floor, &collision.floors),
        (Role::Solid, &collision.solids),
    )
}

pub fn decode_model(bytes: &[u8]) -> Result<ModelCollision> {
    let (floors, solids) = decode_roles(bytes, Role::Floor, Role::Solid)?;
    Ok(ModelCollision { floors, solids })
}

#[cfg(test)]
mod tests;
