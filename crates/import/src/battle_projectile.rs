//! Decode 400-byte projectile templates.
mod contact_path;
use crate::read::Field;
use anyhow::{Context, Result, ensure};
pub(crate) use resonance_content::battle_enemy::Projectile as EnemyProjectile;
use resonance_content::battle_projectile::*;

// Allocated point-history mode is not submitted; explicit birth and trail effects remain visible.
const POINT_HISTORY: u32 = 0x4000;
// Standard projectile motion does not use this declaration bit.
const UNUSED_STANDARD_FLAG: u32 = 0x10;

fn record(row: &[u8]) -> Result<Projectile> {
    record_with_scale(row, None)
}

fn record_with_scale(row: &[u8], reset_scale: Option<f32>) -> Result<Projectile> {
    ensure!(row.len() == 400, "invalid projectile record size");
    let effect = |at| Effect {
        bank: row[at],
        member: row[at + 1],
    };
    let flags = u32::read(row, 8)? & !UNUSED_STANDARD_FLAG;
    let auxiliary_enabled = flags & POINT_HISTORY != 0;
    let plain_layout = [
        0..8,
        0x10..0x11,
        0x18..0x24,
        0x58..0x5c,
        0x93..0x94,
        0x9d..400,
    ]
    .into_iter()
    .all(|range| row[range].iter().all(|&byte| byte == 0));
    let bone_launch = flags & 0x20000 != 0 && row[0x9a] != row[0x9b] && plain_layout;
    // Ballistic weapon shots retain their authored heading-relative velocity.
    // The emission record supplies their launch point independently.
    let ballistic_weapon =
        flags == 0x7209 && row[0x9c] == 4 && row[0x9a] != 0 && row[0x9b] != 0 && plain_layout;
    let ground_ricochet = flags == 0xf209 && row[0x9c] == 2 && row[0x8c..0x90] == [0; 4];
    let flight_flags = if ground_ricochet {
        flags & !0x8000
    } else {
        flags
    };
    let departure = (flags == 0x02001049 && row[0x9c] == 0 && row[0x14..0x18] == [1, 0, 0, 0])
        .then_some(ProjectileDeparture {
            return_speed: 4.,
            jump_speed: 10.,
            ground_speed: 12.,
            ground_reference: [1., 0.],
        });
    // Converging lances choose their direction once, then stop at the floor.
    let ground_lance = flags == 0xd0609
        && row[0x9c] == 4
        && row[0x14..0x18] == [1, 0, 1, 0]
        && [
            0..8,
            0xe..0x11,
            0x18..0x3c,
            0x4c..0x5c,
            0x60..0x84,
            0x93..0x9c,
            0x9d..400,
        ]
        .into_iter()
        .all(|range| row[range].iter().all(|&byte| byte == 0));
    // Missile artwork is supplied by the explicit birth model. Its allocated
    // point-history mode is not submitted; keep other controller settings strict.
    let model_missile = plain_layout
        && row[0x4c..0x5c] == [0; 16]
        && row[0x6c..0x88] == [0; 28]
        && row[0x90..0x94] == [0; 4]
        && row[0x9a..0x9c] == [0; 2]
        && row[0x9d] == 0
        && match (flags, row[0x9c]) {
            (0x4201, 2) => {
                row[0x14..0x18] == [0; 4] && row[0x60..0x6c] == [0; 12] && row[0x94..0x9a] == [0; 6]
            }
            (0x16005, 6) => row[0x14..0x18] == [0, 0, 1, 0] && row[0x30..0x3c] == [0; 12],
            _ => false,
        };
    // Ordinary flights retain explicit artwork; the allocated history mode is not submitted.
    let ordinary_motion = matches!(
        (flight_flags, row[0x9c]),
        (0x4209 | 0x7209, 0) | (0x620a, 2) | (0x214209, 4)
    ) || (matches!(
        (flight_flags, row[0x9c]),
        (0x4209 | 0x6209 | 0x7209 | 0x7229, 2)
    ) && row[0x8c..0x90] == [0; 4])
        || model_missile
        || ballistic_weapon
        || (bone_launch && (!auxiliary_enabled || row[0x8c..0x90] == [0; 4]));
    // These directed layouts initialize their contact-enabled byte to one.
    let directed_state = matches!(
        (flags, row[0x9c], row[0x14]),
        (0x620a, 2, 1) | (0x214209, 4, 1)
    );
    // Bouncing templates keep their auxiliary line color separate from the
    // prepared birth, periodic, and floor effect programs.
    let bouncing_motion = matches!(
        (flags, row[0x9c]),
        (0x1029 | 0x1429 | 0x1002a, 4) | (0x1409, 0)
    ) && row[0x14] <= 1
        && <[u8; 4]>::read(row, 0x8c)? == [128, 64, 64, 255];
    let mut unsupported_reason = (bone_launch && auxiliary_enabled && row[0x8c..0x90] != [0; 4])
        .then(|| "projectile point-history drawing is not prepared".into());
    if (flags & 0x101020 != 0 || ordinary_motion)
        && [
            0..8,
            0x10..0x11,
            0x14..0x15,
            0x18..0x24,
            0x58..0x5c,
            0x8c..0x90,
            0x93..0x94,
            0x9e..400,
        ]
        .into_iter()
        .filter(|range| {
            range.start != 0x8c || (auxiliary_enabled && !ordinary_motion && !bouncing_motion)
        })
        .filter(|range| {
            !(directed_state || bouncing_motion || departure.is_some()) || range.start != 0x14
        })
        .any(|range| row[range].iter().any(|&byte| byte != 0))
    {
        unsupported_reason = Some("projectile response requires unsupported instance state".into());
    }
    let flags = if ordinary_motion {
        flags & !POINT_HISTORY
    } else {
        flags
    };
    if flags & 0x20000 != 0 && !bone_launch {
        unsupported_reason
            .get_or_insert_with(|| "projectile bone targeting is not prepared".into());
    }
    let mut scalar = |offset, active| {
        let value = f32::from_bits(u32::read(row, offset).expect("validated projectile size"));
        if value.is_finite() {
            value
        } else {
            if active {
                unsupported_reason
                    .get_or_insert_with(|| format!("non-finite projectile value at {offset:#x}"));
            }
            0.
        }
    };
    let velocity = std::array::from_fn(|index| scalar(0x24 + index * 4, true));
    let acceleration = std::array::from_fn(|index| scalar(0x30 + index * 4, true));
    let speed = (flags & 0x10000 != 0).then(|| scalar(0x3c, true));
    let response = (flags & 0x101020 != 0 && departure.is_none()).then(|| ProjectileResponse {
        bounce: flags & 0x20 != 0,
        ricochet: flags & 0x1000 != 0,
        ricochet_on_deflect: flags & 0x100000 != 0,
        ground_ricochet,
        restitution: scalar(0x40, true),
    });
    let radius = scalar(0x44, true);
    let height = scalar(0x48, true);
    let width = scalar(0x4c, row[0x15] == 3);
    let growth = std::array::from_fn(|index| scalar(0x50 + index * 4, true));
    let spawn_offset = std::array::from_fn(|index| scalar(0x60 + index * 4, true));
    let velocity_jitter = std::array::from_fn(|index| scalar(0x6c + index * 4, true));
    let hit_offset = std::array::from_fn(|index| scalar(0x78 + index * 4, true));
    let steering = (flags & 0x400004 != 0).then(|| ProjectileSteering {
        blend: scalar(0x94, true),
        start: u32::from(row[0x99]),
        end: (row[0x98] != 0).then_some(u32::from(row[0x98])),
        planar: flags & 0x400000 != 0,
    });
    let stationary = velocity == [0.; 3] && acceleration == [0.; 3] && velocity_jitter == [0.; 3];
    let stationary_motion = flags == 0x409
        && matches!(row[0x9c], 4 | 10)
        && stationary
        && plain_layout
        && row[0x14] <= 1
        && matches!(row[0x15], 0 | 1 | 4)
        && row[0x94..0x9c] == [0; 8]
        && if row[0x9c] == 4 {
            row[0xf] == 0 && row[0x5d] == 0 && row[0x5f] == 0 && row[0x84..0x86] == [0; 2]
        } else {
            row[0x15] == 0 && row[0xf] == 0 && row[0x5f] == 0
        };
    let scattered_motion = flags == 1
        && row[0x9c] == 5
        && plain_layout
        && acceleration == [0.; 3]
        && row[0x14..0x18] == [0; 4]
        && row[0xf] == 0
        && row[0x94..0x9c] == [0; 8];
    let target_floor = flags == 0x489
        && matches!(row[0x9c], 0 | 2)
        && stationary
        && plain_layout
        && row[0x94..0x9c] == [0; 8]
        && (row[0x9c] == 0 || (row[0x14..0x18] == [0, 1, 1, 0] && row[0x8c..0x90] == [0; 4]));
    let moving_contact = flags == 0x409
        && row[0x9c] == 4
        && plain_layout
        && row[0x14..0x18] == [0; 4]
        && acceleration == [0.; 3]
        && velocity_jitter == [0.; 3]
        && growth == [0.; 2]
        && row[0xf] == 0
        && row[0x5f] == 0
        && row[0x84..0x88] == [0; 4]
        && row[0x94..0x9c] == [0; 8];
    let emitted_sphere = matches!(flags, 0xc10001 | 0x800001)
        && row[0x9c] == 4
        && plain_layout
        && row[0x14..0x18] == [1, 4, 1, 0]
        && growth == [0.; 2]
        && spawn_offset == [0.; 3]
        && hit_offset == [0.; 3]
        && row[0xf] == 0
        && row[0x5d] == 0
        && row[0x84..0x88] == [0; 4]
        && row[0x98..0x9c] == [0; 4];
    let planar_homing = emitted_sphere
        && flags == 0xc10001
        && stationary
        && steering.is_some_and(|steering| steering.blend == 90.);
    let scattered_sphere = emitted_sphere && flags == 0x800001 && row[0x94..0x98] == [0; 4];
    // These flights integrate the authored velocity and acceleration directly;
    // contact growth and periodic artwork remain separate prepared behaviors.
    let ballistic_motion = flags == 9
        && row[0x9c] == 4
        && row[0x14] <= 1
        && matches!(row[0x15], 0 | 4)
        && velocity_jitter == [0.; 3]
        && plain_layout;
    if !matches!(row[0x9c], 0 | 2)
        && !(bouncing_motion
            || ground_lance
            || ordinary_motion
            || ballistic_motion
            || stationary_motion
            || scattered_motion
            || moving_contact
            || planar_homing
            || scattered_sphere)
    {
        unsupported_reason.get_or_insert_with(|| "projectile update mode is not prepared".into());
    }
    let mut unsupported = |reason: &str| {
        unsupported_reason.get_or_insert_with(|| reason.into());
    };
    if flags & !0x1f9367f != 0
        && !ground_lance
        && departure.is_none()
        && !bone_launch
        && !target_floor
        && !ground_ricochet
    {
        unsupported("projectile motion/effect controller is not prepared");
    }
    let velocity_reset = if row[0x9d] == 0 {
        None
    } else if let Some(acceleration_scale) = reset_scale.filter(|scale| scale.is_finite()) {
        if speed.is_some() {
            unsupported("directed projectile cannot reset ballistic velocity");
        }
        Some(ProjectileVelocityReset {
            age: u32::from(row[0x9d]),
            acceleration_scale,
        })
    } else {
        unsupported("projectile velocity reset scale is not prepared");
        None
    };
    let ground_effect = effect(0xe);
    let trail_effect = effect(0x5e);
    let ground_response = flags == 0x2061
        && row[0x9c] == 2
        && plain_layout
        && row[0x14..0x18] == [0; 4]
        && row[0x8c..0x9c] == [0; 16];
    if response.is_some()
        && (flags & !(0x11366b | if ground_ricochet { 0x8000 } else { 0 }) != 0
            || (response.is_some_and(|response| response.ricochet || response.ricochet_on_deflect)
                && speed.is_some())
            || (ground_effect.member != 0 && !bouncing_motion && !ground_response))
    {
        unsupported("projectile response combination is not prepared");
    }
    let shape = match row[0x15] {
        0 => HitShape::Box,
        1 => HitShape::Cylinder,
        2 => HitShape::GroundCircle,
        3 => HitShape::Ring { width },
        4 => HitShape::Sphere,
        _ => {
            unsupported("projectile contact shape is not prepared");
            HitShape::Box
        }
    };
    let lifetime = i16::read(row, 0xc)?;
    if lifetime < 0 {
        unsupported("invalid projectile lifetime");
    }
    let lifetime = (lifetime > 0).then_some(lifetime as u32);
    let start = i16::read(row, 0x84)?;
    let duration = i16::read(row, 0x86)?;
    let active = if duration == 0 {
        None
    } else if start >= 0 && duration > 0 {
        Some([start as u32, start as u32 + duration as u32])
    } else {
        unsupported("invalid projectile active interval");
        None
    };
    let interval = i16::read(row, 0x90)?;
    let trail_interval = if trail_effect.member == 0 {
        None
    } else if row[0x92] == 4 && interval > 0 {
        Some(interval as u32)
    } else {
        unsupported("projectile periodic effect controller is not prepared");
        None
    };
    if speed.is_some_and(|speed| speed <= 0.) {
        unsupported("invalid projectile speed");
    }
    if steering.is_some_and(|steering| {
        (!(0.0..=1.0).contains(&steering.blend) && !planar_homing)
            || steering.end.is_some_and(|end| steering.start >= end)
    }) {
        unsupported("invalid projectile steering");
    }
    Ok(Projectile {
        lifetime,
        velocity,
        acceleration,
        spawn_offset,
        clamp_ground: flags & 0x40 != 0,
        active,
        motion: ProjectileMotion {
            origin: if target_floor {
                ProjectileOrigin::TargetFloor
            } else {
                ProjectileOrigin::Actor
            },
            bone_direction: bone_launch.then_some(ProjectileBoneDirection {
                from: row[0x9b],
                to: row[0x9a],
            }),
            departure,
            velocity_reset,
            world_direction: flags & 0x200000 != 0,
            face_velocity: flags & 0x200 != 0,
            aim_at_target: ground_lance,
            stop_on_ground: flags & 0x80000 != 0,
            contact_path: Vec::new(),
            unlimited_range: flags & 0x1000000 != 0,
            aqua_edge: false,
            velocity_jitter,
            speed,
            steering,
            response,
        },
        contact: Contact {
            shape,
            recoil_direction: match row[0x16] {
                0 => RecoilDirection::Travel,
                1 => RecoilDirection::AwayFromOwner,
                2 => RecoilDirection::AwayFromContact,
                3 => RecoilDirection::TowardContact,
                _ => RecoilDirection::None,
            },
            repeat_limit: row[0x17],
            radius,
            height,
            growth,
            offset: hit_offset,
            survives_contact: flags & 8 != 0,
            clashes: flags & 0x2000 != 0,
        },
        birth_effect: effect(0x5c),
        ground_effect,
        trail_effect,
        trail_interval,
        shadow: (flags & 0x400 == 0).then_some(ProjectileShadow {
            color: <[u8; 4]>::read(row, 0x88)?,
            additive: flags & 0x800000 != 0,
            radius,
        }),
        unsupported_reason,
    })
}

pub(crate) fn read(bytes: &[u8]) -> Result<Table> {
    read_with_scale(bytes, None)
}

fn read_with_scale(bytes: &[u8], reset_scale: Option<f32>) -> Result<Table> {
    ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(400),
        "misaligned projectile table"
    );
    Ok(Table {
        source_sha256: crate::digest(bytes),
        records: bytes
            .chunks_exact(400)
            .map(|row| record_with_scale(row, reset_scale))
            .collect::<Result<_>>()?,
        resident_spells: Default::default(),
        support_durations: Default::default(),
        fire_ball: None,
    })
}

/// Bind model contacts only when the enemy package supplies a proven recipe.
pub(crate) fn read_enemy_member(
    bytes: &[u8],
    start: usize,
    members: &crate::model_preview::PointerMembers<'_>,
) -> Result<Vec<EnemyProjectile>> {
    let mut table = read_package_member(bytes, start)?;
    for (row, projectile) in bytes.chunks_exact(400).zip(&mut table.records) {
        let flags = u32::read(row, 8)?;
        if flags & 0x800 == 0 {
            continue;
        }
        // Keep ordinary decoding strict; admission requires the bound contact path.
        let mut ordinary = row.to_vec();
        ordinary[8..12].copy_from_slice(&(flags & !0x800).to_be_bytes());
        let mut prepared = record(&ordinary)?;
        if prepared.unsupported_reason.is_some() {
            continue;
        }
        match contact_path::prepare(row, &prepared, members) {
            Ok(path) => {
                prepared.motion.contact_path = path;
                *projectile = prepared;
            }
            Err(error) => {
                projectile.unsupported_reason =
                    Some(format!("projectile model-attachment contact: {error:#}"));
            }
        }
    }
    Ok(table
        .records
        .into_iter()
        .zip(bytes.chunks_exact(400))
        .map(|(definition, row)| EnemyProjectile {
            definition,
            recoil: row[0x13],
        })
        .collect())
}

/// Change a model's birth effect and bake that model's matching contact path.
pub(crate) fn with_model_effect(
    row: &[u8],
    projectile: &Projectile,
    members: &crate::model_preview::PointerMembers<'_>,
    member: u8,
) -> Result<Projectile> {
    ensure!(
        projectile.unsupported_reason.is_none() && u32::read(row, 8)? & 0x800 != 0,
        "model effect variant needs a prepared model contact"
    );
    let mut projectile = projectile.clone();
    projectile.birth_effect.member = member;
    projectile.motion.contact_path.clear();
    projectile.motion.contact_path = contact_path::prepare(row, &projectile, members)?;
    Ok(projectile)
}

/// Bake a shot's authored overrides into its independent projectile recipe.
pub(crate) fn with_modifier(
    shot: &EnemyProjectile,
    bytes: &[u8],
    offset: u16,
) -> Result<EnemyProjectile> {
    let mut shot = shot.clone();
    if offset == 0 {
        return Ok(shot);
    }
    ensure!(
        shot.definition.motion.contact_path.is_empty(),
        "modifiers on model contacts are not prepared"
    );
    let mut at = usize::from(offset);
    loop {
        let operation = u16::read(bytes, at)?;
        if operation == 0 {
            return Ok(shot);
        }
        let target = u16::read(bytes, at + 2)?;
        match (operation, target) {
            (1, 0x24 | 0x30 | 0x60 | 0x6c) => {
                let value = <[f32; 3]>::read(bytes, at + 4)?;
                *match target {
                    0x24 => &mut shot.definition.velocity,
                    0x30 => &mut shot.definition.acceleration,
                    0x60 => &mut shot.definition.spawn_offset,
                    _ => &mut shot.definition.motion.velocity_jitter,
                } = value;
                at += 16;
            }
            (2, 0x13) => {
                shot.recoil = u32::read(bytes, at + 4)?
                    .try_into()
                    .context("projectile recoil exceeds byte range")?;
                at += 8;
            }
            (4, 0x24 | 0x2c | 0x30 | 0x60 | 0x68) => {
                *match target {
                    0x24 => &mut shot.definition.velocity[0],
                    0x2c => &mut shot.definition.velocity[2],
                    0x30 => &mut shot.definition.acceleration[0],
                    0x60 => &mut shot.definition.spawn_offset[0],
                    _ => &mut shot.definition.spawn_offset[2],
                } = f32::read(bytes, at + 4)?;
                at += 8;
            }
            (5, 0x5d) => {
                shot.definition.birth_effect.member = u32::read(bytes, at + 4)?
                    .try_into()
                    .context("projectile effect member exceeds byte range")?;
                at += 8;
            }
            _ => anyhow::bail!(
                "projectile modifier operation {operation} at {target:#x} is not prepared"
            ),
        }
    }
}

/// Enemy package members end at the next declared pointer. They contain
/// 400-byte rows; the following member may be aligned to 32 bytes in the package.
/// Alignment slack may contain nonzero bytes.
pub(crate) fn read_package_member(bytes: &[u8], start: usize) -> Result<Table> {
    let length = bytes.len() / 400 * 400;
    let end = start
        .checked_add(bytes.len())
        .context("projectile member range overflow")?;
    let aligned_end = start
        .checked_add(length)
        .and_then(|end| end.checked_next_multiple_of(32))
        .context("projectile member alignment overflow")?;
    ensure!(
        start.is_multiple_of(4) && length != 0 && (length == bytes.len() || end == aligned_end),
        "projectile member at {start:#x} has {} bytes outside its 400-byte rows and package alignment",
        bytes.len() - length
    );
    let mut table = read(&bytes[..length])?;
    // Hash the complete member, including alignment bytes.
    table.source_sha256 = crate::digest(bytes);
    Ok(table)
}

pub(crate) fn read_resident(usual: &[u8], module: &crate::rel::Rel) -> Result<Table> {
    let reset_scale = f32::read(module.at((4, 0xce8))?, 0)?;
    let mut table = read_with_scale(crate::source_assets::section(usual, 7)?, Some(reset_scale))?;
    let parameters = crate::source_assets::section(usual, 9)?;
    table.resident_spells = resident_spells(parameters)?;
    let fire_ball = module.at((4, 0x3928))?;
    table.fire_ball = Some(FireBall {
        velocities: <[[f32; 3]; 3]>::read(fire_ball, 0)?,
        height_offset: f32::read(fire_ball, 36)?,
        center_height_scale: f32::read(fire_ball, 40)?,
    });
    for spell in [239_u16, 242, 245, 248, 254, 273, 274, 275] {
        let index = usize::from(spell - 200);
        ensure!(
            index < u32::read(parameters, 0)? as usize,
            "missing support spell"
        );
        let start = u32::read(parameters, 4 + index * 4)? as usize;
        let duration = u16::read(parameters, start + 16)?;
        ensure!(duration > 0, "empty support spell duration");
        table.support_durations.insert(spell, duration);
    }
    table.source_sha256 = crate::digest(usual);
    Ok(table)
}

fn resident_spells(bytes: &[u8]) -> Result<std::collections::BTreeMap<u16, ResidentSpell>> {
    let count = u32::read(bytes, 0)? as usize;
    // Spell contacts use their own recoil profiles with the resident damage table.
    [
        (200_u16, 6),
        (204, 1),
        (208, 6),
        (212, 6),
        (216, 1),
        (220, 10),
    ]
    .into_iter()
    .map(|(spell, recoil)| {
        let index = usize::from(spell - 200);
        ensure!(
            index < count,
            "resident spell {spell} is outside its directory"
        );
        let start = u32::read(bytes, 4 + index * 4)? as usize;
        let parameters = bytes
            .get(start..)
            .context("missing resident spell parameters")?;
        let begin = u32::read(parameters, 0)? as usize;
        let end = u32::read(parameters, 4)? as usize;
        let rules = parameters
            .get(begin..end)
            .context("missing resident spell hit rules")?;
        Ok((
            spell,
            ResidentSpell {
                duration: u16::read(parameters, 16)?,
                hit: crate::battle_action::hit_rule(rules, 0, DamageKind::Magic, recoil)?,
                followup_hit: if spell == 220 {
                    Some(crate::battle_action::hit_rule(
                        rules.get(28..56).context("missing Icicle follow-up rule")?,
                        0,
                        DamageKind::Magic,
                        7,
                    )?)
                } else {
                    None
                },
            },
        ))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projectile_scalar_modifiers_preserve_unmodified_components_and_other_shots() -> Result<()> {
        let mut definition = record(&[0; 400])?;
        definition.velocity = [0., 2., -8.];
        definition.acceleration = [0., -0.1, 0.5];
        definition.spawn_offset = [0., 250., -100.];
        let source = EnemyProjectile {
            definition,
            recoil: 0,
        };
        let stream = |values: &[(u16, f32)]| {
            let mut bytes = vec![0; 4];
            for (field, value) in values {
                bytes.extend(4_u16.to_be_bytes());
                bytes.extend(field.to_be_bytes());
                bytes.extend(value.to_be_bytes());
            }
            bytes.extend(0_u16.to_be_bytes());
            bytes
        };
        for (velocity, acceleration) in [(16., -0.4), (-16., 0.4), (18., -0.45), (-18., 0.45)] {
            let modified = with_modifier(
                &source,
                &stream(&[(0x24, velocity), (0x30, acceleration)]),
                4,
            )?;
            assert_eq!(modified.definition.velocity, [velocity, 2., -8.]);
            assert_eq!(modified.definition.acceleration, [acceleration, -0.1, 0.5]);
            assert_eq!(modified.definition.spawn_offset, [0., 250., -100.]);
        }
        let radial = with_modifier(
            &source,
            &stream(&[(0x60, 100.), (0x68, 0.), (0x24, 15.), (0x2c, 0.)]),
            4,
        )?;
        assert_eq!(radial.definition.spawn_offset, [100., 250., 0.]);
        assert_eq!(radial.definition.velocity, [15., 2., 0.]);
        assert_eq!(
            radial.definition.acceleration,
            source.definition.acceleration
        );
        assert_eq!(source.definition.velocity, [0., 2., -8.]);
        assert_eq!(source.definition.spawn_offset, [0., 250., -100.]);
        assert!(with_modifier(&source, &stream(&[(0x34, 1.)]), 4).is_err());
        Ok(())
    }

    #[test]
    fn decodes_semantics_and_marks_unsupported_templates_without_losing_other_rows() -> Result<()> {
        let mut bytes = vec![0; 800];
        bytes[8..12].copy_from_slice(&0x20_u32.to_be_bytes());
        bytes[0xc..0xe].copy_from_slice(&45_i16.to_be_bytes());
        bytes[0x24..0x28].copy_from_slice(&3_f32.to_be_bytes());
        bytes[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
        let table = read(&bytes)?;
        assert_eq!(table.records[0].velocity, [3., 0., 0.]);
        assert_eq!(table.records[0].lifetime, Some(45));
        assert_eq!(table.records[0].motion.response.unwrap().restitution, 0.75);
        assert!(table.records[0].unsupported_reason.is_none());
        for flags in [0x4209_u32, 0x7209] {
            let mut shot = bytes[..400].to_vec();
            shot[8..12].copy_from_slice(&(flags & !0x4000).to_be_bytes());
            let ordinary = record(&shot)?;
            shot[8..12].copy_from_slice(&flags.to_be_bytes());
            shot[0x8c..0x90].copy_from_slice(&[48, 48, 96, 255]);
            assert_eq!(record(&shot)?, ordinary);
            for (at, value) in [(0x9c, 2), (0xb2, 1)] {
                let mut unsupported = shot.clone();
                unsupported[at] = value;
                assert!(record(&unsupported)?.unsupported_reason.is_some());
            }
        }
        for flags in [0x3449_u32, 0x3469] {
            let mut shot = bytes[..400].to_vec();
            shot[8..12].copy_from_slice(&flags.to_be_bytes());
            let ordinary = record(&shot)?;
            assert!(ordinary.unsupported_reason.is_none());
            shot[0x8c..0x90].copy_from_slice(&[64, 128, 64, 255]);
            assert_eq!(record(&shot)?, ordinary);
            shot[8..12].copy_from_slice(&(flags | 0x4000).to_be_bytes());
            assert_eq!(
                record(&shot)?.unsupported_reason.as_deref(),
                Some("projectile response requires unsupported instance state")
            );
        }
        for (flags, mode, speed, blend, end) in [
            (0x4211_u32, 2, None, 0., 0),
            (0x16015, 6, Some(10.5_f32), 0.25_f32, 20),
            (0x16015, 6, Some(20.), 0.19, 19),
        ] {
            let mut shot = [0; 400];
            shot[8..12].copy_from_slice(&flags.to_be_bytes());
            shot[0xc..0xe].copy_from_slice(&250_u16.to_be_bytes());
            shot[0xe..0x10].copy_from_slice(&[2, 9]);
            shot[0x16] = u8::from(speed.is_some());
            shot[0x28..0x2c].copy_from_slice(&3_f32.to_be_bytes());
            shot[0x2c..0x30].copy_from_slice(&0.5_f32.to_be_bytes());
            shot[0x44..0x48].copy_from_slice(&20_f32.to_be_bytes());
            shot[0x48..0x4c].copy_from_slice(&20_f32.to_be_bytes());
            shot[0x5c..0x5e].copy_from_slice(&[2, 7]);
            shot[0x88..0x8c].copy_from_slice(&[0, 0, 0, 128]);
            shot[0x8c..0x90].copy_from_slice(&[96, 32, 16, 128]);
            shot[0x9c] = mode;
            if let Some(speed) = speed {
                shot[0x3c..0x40].copy_from_slice(&speed.to_be_bytes());
                shot[0x60..0x64].copy_from_slice(&102_f32.to_be_bytes());
                shot[0x94..0x98].copy_from_slice(&blend.to_be_bytes());
                shot[0x98..0x9a].copy_from_slice(&[end, 5]);
            } else {
                shot[0x34..0x38].copy_from_slice(&(-0.5_f32).to_be_bytes());
            }
            let missile = record(&shot)?;
            assert!(missile.unsupported_reason.is_none());
            assert_eq!(missile.velocity, [0., 3., 0.5]);
            assert_eq!(
                missile.acceleration[1],
                if speed.is_some() { 0. } else { -0.5 }
            );
            assert_eq!(missile.motion.speed, speed);
            assert_eq!(
                missile
                    .motion
                    .steering
                    .map(|value| (value.blend, value.start, value.end)),
                speed.map(|_| (blend, 5, Some(u32::from(end))))
            );
            assert_eq!(
                missile.spawn_offset[0],
                if speed.is_some() { 102. } else { 0. }
            );
            assert_eq!(missile.birth_effect, Effect { bank: 2, member: 7 });
            assert_eq!(missile.ground_effect, Effect { bank: 2, member: 9 });
            assert_eq!(missile.shadow.unwrap().color, [0, 0, 0, 128]);
            shot[0x8c..0x90].fill(0);
            assert_eq!(record(&shot)?, missile);
            for at in [0x14, 0x18, 0x50, 0x84, 0x9a, 0x9d, 0xb2] {
                let mut unsupported = shot;
                unsupported[at] = 1;
                assert!(record(&unsupported)?.unsupported_reason.is_some());
            }
            shot[8..12].copy_from_slice(&(flags | 0x100000).to_be_bytes());
            assert!(record(&shot)?.unsupported_reason.is_some());
        }
        let mut sylph = [0; 400];
        sylph[8..12].copy_from_slice(&0x220a_u32.to_be_bytes());
        sylph[0x14] = 1;
        sylph[0x9c] = 2;
        sylph[0x5c..0x5e].copy_from_slice(&[2, 3]);
        let ordinary = record(&sylph)?;
        assert!(ordinary.unsupported_reason.is_none());
        sylph[8..12].copy_from_slice(&0x620a_u32.to_be_bytes());
        assert_eq!(record(&sylph)?, ordinary);
        for (at, value) in [(0x14, 2), (0x9c, 0), (0xb2, 1)] {
            let mut unsupported = sylph;
            unsupported[at] = value;
            assert!(record(&unsupported)?.unsupported_reason.is_some());
        }
        let mut other_layout = sylph;
        other_layout[8..12].copy_from_slice(&0x4209_u32.to_be_bytes());
        other_layout[0x9c] = 0;
        assert!(record(&other_layout)?.unsupported_reason.is_some());
        let mut sphere = [0; 400];
        sphere[8..12].copy_from_slice(&9_u32.to_be_bytes());
        sphere[0x15] = 4;
        sphere[0x50..0x54].copy_from_slice(&10_f32.to_be_bytes());
        sphere[0x54..0x58].copy_from_slice(&10_f32.to_be_bytes());
        sphere[0x8c..0x90].copy_from_slice(&[32, 32, 128, 128]);
        let ordinary = record(&sphere)?;
        sphere[0x9c] = 4;
        assert_eq!(record(&sphere)?, ordinary);
        for (at, value) in [(0x24, 0x3f), (0x30, 0x3f), (0x15, 0), (0x5d, 1)] {
            let mut ballistic = sphere;
            ballistic[at] = value;
            assert!(record(&ballistic)?.unsupported_reason.is_none());
        }
        for (at, value) in [(0x15, 5), (0x5f, 1), (0xb2, 1)] {
            let mut unsupported = sphere;
            unsupported[at] = value;
            assert!(record(&unsupported)?.unsupported_reason.is_some());
        }
        bytes[0xb2] = 1;
        let table = read(&bytes)?;
        assert!(table.records[0].unsupported_reason.is_some());
        assert!(table.records[1].unsupported_reason.is_none());
        bytes[0xb2] = 0;
        bytes[0x9c] = 3;
        assert!(read(&bytes)?.records[0].unsupported_reason.is_some());
        bytes[0x9c] = 0;
        bytes[0x24..0x28].copy_from_slice(&f32::NAN.to_be_bytes());
        let table = read(&bytes)?;
        assert!(table.records[0].unsupported_reason.is_some());
        assert_eq!(table.records[0].velocity[0], 0.);
        assert!(serde_json::to_vec(&table).is_ok());
        assert!(read(&[]).is_err());
        assert!(read(&bytes[..799]).is_err());
        Ok(())
    }

    #[test]
    fn disabled_template_fields_are_discarded() -> Result<()> {
        let mut bytes = [0; 400];
        let expected = record(&bytes)?;
        for offset in [0x3c, 0x40, 0x4c, 0x94] {
            bytes[offset..offset + 4].copy_from_slice(&f32::NAN.to_be_bytes());
        }
        bytes[0x9a..0x9c].copy_from_slice(&[1, 2]);
        assert_eq!(record(&bytes)?, expected);
        Ok(())
    }

    #[test]
    fn projectile_clocks_reject_negative_durations_and_keep_wide_interval_ends() -> Result<()> {
        let mut bytes = [0; 400];
        assert!(record(&bytes)?.lifetime.is_none());
        bytes[0x84..0x86].copy_from_slice(&32767_i16.to_be_bytes());
        bytes[0x86..0x88].copy_from_slice(&2_i16.to_be_bytes());
        assert_eq!(record(&bytes)?.active, Some([32767, 32769]));
        for offset in [0xc, 0x86] {
            let mut invalid = bytes;
            invalid[offset..offset + 2].copy_from_slice(&(-1_i16).to_be_bytes());
            assert!(record(&invalid)?.unsupported_reason.is_some());
        }
        Ok(())
    }

    #[test]
    fn specialized_controllers_remain_unavailable_after_decoding() -> Result<()> {
        for flags in [0x8000_u32, 0x40000, 0x20000, 0x10020, 0x24] {
            let mut bytes = [0; 400];
            bytes[8..12].copy_from_slice(&flags.to_be_bytes());
            assert!(
                record(&bytes)?.unsupported_reason.is_some(),
                "flags {flags:#x}"
            );
        }
        for (at, value) in [(0x9d, 1), (0x15, 5), (0x5f, 1)] {
            let mut bytes = [0; 400];
            bytes[at] = value;
            assert!(
                record(&bytes)?.unsupported_reason.is_some(),
                "field {at:#x}"
            );
        }
        let mut bytes = [0; 400];
        bytes[8..12].copy_from_slice(&0x280209_u32.to_be_bytes());
        bytes[0x9d] = 6;
        let unbound = record(&bytes)?;
        assert_eq!(
            unbound.unsupported_reason.as_deref(),
            Some("projectile velocity reset scale is not prepared")
        );
        let prepared = record_with_scale(&bytes, Some(0.25))?;
        assert!(prepared.unsupported_reason.is_none());
        assert!(
            prepared.motion.world_direction
                && prepared.motion.face_velocity
                && prepared.motion.stop_on_ground
        );
        assert_eq!(
            prepared.motion.velocity_reset,
            Some(ProjectileVelocityReset {
                age: 6,
                acceleration_scale: 0.25
            })
        );
        Ok(())
    }

    #[test]
    fn audited_launches_and_updates_preserve_unproved_controller_gates() -> Result<()> {
        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0x409_u32.to_be_bytes());
        for mode in [4, 10] {
            row[0x9c] = mode;
            let projectile = record(&row)?;
            assert!(projectile.unsupported_reason.is_none());
            assert_eq!(projectile.velocity, [0.; 3]);
            row[0x30..0x34].copy_from_slice(&1_f32.to_be_bytes());
            assert!(record(&row)?.unsupported_reason.is_some());
            row[0x30..0x34].fill(0);
        }
        row[8..12].copy_from_slice(&1_u32.to_be_bytes());
        row[0x9c] = 5;
        row[0x2c..0x30].copy_from_slice(&15_f32.to_be_bytes());
        row[0x6c..0x70].copy_from_slice(&10_f32.to_be_bytes());
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.velocity, [0., 0., 15.]);
        assert_eq!(projectile.motion.velocity_jitter, [10., 0., 0.]);
        row[0x9c] = 6;
        assert!(record(&row)?.unsupported_reason.is_some());

        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0x30409_u32.to_be_bytes());
        row[0x9c] = 2;
        row[0x9a..0x9c].copy_from_slice(&[13, 11]);
        row[0x3c..0x40].copy_from_slice(&30_f32.to_be_bytes());
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.motion.origin, ProjectileOrigin::Actor);
        assert_eq!(
            projectile.motion.bone_direction,
            Some(ProjectileBoneDirection { from: 11, to: 13 })
        );
        assert_eq!(projectile.motion.speed, Some(30.));
        row[8..12].copy_from_slice(&0x34409_u32.to_be_bytes());
        assert_eq!(record(&row)?, projectile);
        row[0x8f] = 128;
        assert!(record(&row)?.unsupported_reason.is_some());

        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0x7209_u32.to_be_bytes());
        row[0x9c] = 4;
        row[0x9a..0x9c].copy_from_slice(&[46, 61]);
        row[0x28..0x2c].copy_from_slice(&(-1_f32).to_be_bytes());
        row[0x2c..0x30].copy_from_slice(&35_f32.to_be_bytes());
        row[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
        row[0x44..0x48].copy_from_slice(&20_f32.to_be_bytes());
        row[0x5c..0x5e].copy_from_slice(&[2, 1]);
        row[0x6c..0x70].copy_from_slice(&0.5_f32.to_be_bytes());
        row[0x70..0x74].copy_from_slice(&1_f32.to_be_bytes());
        row[0x74..0x78].copy_from_slice(&1_f32.to_be_bytes());
        row[0x88..0x8c].copy_from_slice(&[24, 24, 24, 144]);
        row[0x8c..0x90].copy_from_slice(&[64, 64, 128, 128]);
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.motion.origin, ProjectileOrigin::Actor);
        assert!(projectile.motion.bone_direction.is_none());
        assert_eq!(projectile.velocity, [0., -1., 35.]);
        assert_eq!(projectile.motion.velocity_jitter, [0.5, 1., 1.]);
        assert!(projectile.motion.speed.is_none());
        assert_eq!(projectile.birth_effect, Effect { bank: 2, member: 1 });
        assert_eq!(projectile.shadow.as_ref().unwrap().color, [24, 24, 24, 144]);
        let response = projectile.motion.response.unwrap();
        assert!(response.ricochet && !response.bounce && !response.ground_ricochet);
        assert_eq!(response.restitution, 0.75);
        let mut transparent = row;
        transparent[0x8c..0x90].fill(0);
        assert_eq!(record(&transparent)?, projectile);
        for offset in [0x14, 0x18, 0x58, 0x9d, 0xb2] {
            let mut unprepared = row;
            unprepared[offset] = 1;
            assert!(record(&unprepared)?.unsupported_reason.is_some());
        }
        row[8..12].copy_from_slice(&0x107209_u32.to_be_bytes());
        assert!(record(&row)?.unsupported_reason.is_some());

        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0x489_u32.to_be_bytes());
        row[0x64..0x68].copy_from_slice(&500_f32.to_be_bytes());
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.motion.origin, ProjectileOrigin::TargetFloor);
        assert!(projectile.motion.bone_direction.is_none());
        assert_eq!(projectile.spawn_offset, [0., 500., 0.]);
        row[0x64..0x68].fill(0);
        row[0xc..0xe].copy_from_slice(&60_i16.to_be_bytes());
        row[0x14..0x18].copy_from_slice(&[0, 1, 1, 0]);
        row[0x84..0x86].copy_from_slice(&30_i16.to_be_bytes());
        row[0x9c] = 2;
        for (radius, height, duration) in [(160_f32, 40_f32, 2_i16), (200., 100., 32)] {
            row[0x44..0x48].copy_from_slice(&radius.to_be_bytes());
            row[0x48..0x4c].copy_from_slice(&height.to_be_bytes());
            row[0x86..0x88].copy_from_slice(&duration.to_be_bytes());
            let projectile = record(&row)?;
            assert!(projectile.unsupported_reason.is_none());
            assert_eq!(projectile.motion.origin, ProjectileOrigin::TargetFloor);
            assert_eq!(projectile.lifetime, Some(60));
            assert_eq!(projectile.active, Some([30, 30 + duration as u32]));
        }
        row[0x8f] = 255;
        assert!(record(&row)?.unsupported_reason.is_some());
        row[0x8f] = 0;
        row[8..12].copy_from_slice(&0x100489_u32.to_be_bytes());
        assert!(record(&row)?.unsupported_reason.is_some());

        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0xf209_u32.to_be_bytes());
        row[0x9c] = 2;
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        let response = projectile.motion.response.unwrap();
        assert!(response.ricochet && response.ground_ricochet && !response.bounce);
        row[0x8f] = 255;
        assert!(record(&row)?.unsupported_reason.is_some());
        Ok(())
    }

    #[test]
    fn moving_contacts_and_emitted_spheres_keep_authored_motion_and_layout_guards() -> Result<()> {
        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0x409_u32.to_be_bytes());
        row[0x9c] = 4;
        row[0x2c..0x30].copy_from_slice(&30_f32.to_be_bytes());
        row[0x5c..0x5e].copy_from_slice(&[2, 3]);
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.velocity, [0., 0., 30.]);
        assert!(projectile.contact.survives_contact);
        assert_eq!(projectile.birth_effect, Effect { bank: 2, member: 3 });
        for offset in [0x30, 0x6c, 0x50, 0x9a, 0xb2] {
            let mut unprepared = row;
            unprepared[offset] = 1;
            assert!(record(&unprepared)?.unsupported_reason.is_some());
        }

        let mut row = [0; 400];
        row[8..12].copy_from_slice(&0xc10011_u32.to_be_bytes());
        row[0x9c] = 4;
        row[0x14..0x18].copy_from_slice(&[1, 4, 1, 0]);
        row[0x3c..0x40].copy_from_slice(&4_f32.to_be_bytes());
        row[0x94..0x98].copy_from_slice(&90_f32.to_be_bytes());
        row[0x5e..0x60].copy_from_slice(&[2, 3]);
        row[0x90..0x94].copy_from_slice(&[0, 5, 4, 0]);
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.motion.steering.unwrap().blend, 90.);
        assert!(projectile.motion.steering.unwrap().planar);
        assert_eq!(projectile.trail_interval, Some(5));
        for (offset, value) in [(0x94, 45_f32), (0x24, 1.), (0x60, 1.)] {
            let mut unprepared = row;
            unprepared[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            assert!(record(&unprepared)?.unsupported_reason.is_some());
        }

        row[8..12].copy_from_slice(&0x800011_u32.to_be_bytes());
        row[0x94..0x98].fill(0);
        for (offset, value) in [
            (0x28, 26_f32),
            (0x2c, 8.),
            (0x34, -1.),
            (0x6c, 8.),
            (0x70, 4.),
            (0x74, 6.),
        ] {
            row[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        let projectile = record(&row)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.velocity, [0., 26., 8.]);
        assert_eq!(projectile.acceleration, [0., -1., 0.]);
        assert_eq!(projectile.motion.velocity_jitter, [8., 4., 6.]);
        assert!(projectile.motion.steering.is_none());
        row[8..12].copy_from_slice(&0x804011_u32.to_be_bytes());
        assert!(record(&row)?.unsupported_reason.is_some());
        Ok(())
    }

    #[test]
    fn contact_projectiles_reuse_standard_bounce_and_ricochet() -> Result<()> {
        for flags in [0x102229_u32, 0x102249] {
            let mut bytes = [0; 400];
            bytes[8..12].copy_from_slice(&flags.to_be_bytes());
            bytes[0xc..0xe].copy_from_slice(&120_i16.to_be_bytes());
            bytes[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
            bytes[0x5c..0x5e].copy_from_slice(&[2, 1]);
            bytes[0x9c] = 2;
            let projectile = record(&bytes)?;
            assert!(projectile.unsupported_reason.is_none());
            let response = projectile.motion.response.unwrap();
            assert!(response.ricochet_on_deflect && !response.ricochet);
            assert_eq!(response.bounce, flags & 0x20 != 0);
            assert_eq!(projectile.clamp_ground, flags & 0x40 != 0);
            assert_eq!(projectile.lifetime, Some(120));
            assert!(projectile.contact.survives_contact && projectile.contact.clashes);
        }
        let mut ground = [0; 400];
        ground[8..12].copy_from_slice(&0x2061_u32.to_be_bytes());
        ground[0xe..0x10].copy_from_slice(&[2, 3]);
        ground[0x9c] = 2;
        ground[0x86..0x88].copy_from_slice(&74_i16.to_be_bytes());
        let projectile = record(&ground)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.ground_effect, Effect { bank: 2, member: 3 });
        assert!(projectile.motion.response.unwrap().bounce);
        ground[8..12].copy_from_slice(&0x2071_u32.to_be_bytes());
        assert_eq!(record(&ground)?, projectile);
        for (at, value) in [(0x14, 1), (0x8f, 255), (0x9c, 0), (0x5f, 1)] {
            let mut unprepared = ground;
            unprepared[at] = value;
            assert!(record(&unprepared)?.unsupported_reason.is_some());
        }
        for (flags, member) in [(0x3409_u32, 2), (0x3229, 6), (0x3009, 2)] {
            for interval in [1_i16, 3, 6] {
                let mut bytes = [0; 400];
                bytes[8..12].copy_from_slice(&flags.to_be_bytes());
                bytes[0x9c] = 2;
                bytes[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
                bytes[0x5e..0x60].copy_from_slice(&[2, member]);
                bytes[0x90..0x92].copy_from_slice(&interval.to_be_bytes());
                bytes[0x92] = 4;
                let projectile = record(&bytes)?;
                assert!(projectile.unsupported_reason.is_none());
                assert_eq!(projectile.trail_effect, Effect { bank: 2, member });
                assert_eq!(projectile.trail_interval, Some(interval as u32));
                let response = projectile.motion.response.unwrap();
                assert!(response.ricochet);
                assert_eq!(response.bounce, flags & 0x20 != 0);
                assert_eq!(response.restitution, 0.75);
                assert!(projectile.contact.survives_contact && projectile.contact.clashes);
                bytes[8..12].copy_from_slice(&(flags | 0x10).to_be_bytes());
                assert_eq!(record(&bytes)?, projectile);
                for (at, value) in [(0xf, 3), (0x91, 0), (0x92, 3), (0x9c, 7), (0x8, 0x80)] {
                    let mut unprepared = bytes;
                    unprepared[at] = value;
                    assert!(record(&unprepared)?.unsupported_reason.is_some());
                }
            }
        }
        for (lifetime, upward, forward, gravity, effect) in [
            (60_i16, 11_f32, 7_f32, -0.55_f32, 2),
            (80, 16., 12., -0.6, 3),
        ] {
            let mut bytes = [0; 400];
            bytes[8..12].copy_from_slice(&0x3029_u32.to_be_bytes());
            bytes[0xc..0xe].copy_from_slice(&lifetime.to_be_bytes());
            for (at, value) in [
                (0x28, upward),
                (0x2c, forward),
                (0x34, gravity),
                (0x40, 0.75),
                (0x44, 20.),
                (0x48, 20.),
                (0x64, 80.),
                (0x68, 30.),
            ] {
                bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            bytes[0x5c..0x60].copy_from_slice(&[2, effect, 2, 0]);
            let mut standard = record(&bytes)?;
            assert!(standard.unsupported_reason.is_none());
            assert!(!standard.motion.face_velocity);
            bytes[8..12].copy_from_slice(&0x3229_u32.to_be_bytes());
            standard.motion.face_velocity = true;
            assert_eq!(record(&bytes)?, standard);
        }
        for (flags, clashes, bounce, ricochet) in [
            (0x4209_u32, false, false, false),
            (0x6209, true, false, false),
            (0x7209, true, false, true),
            (0x7229, true, true, true),
        ] {
            let mut bytes = [0; 400];
            bytes[8..12].copy_from_slice(&(flags & !0x4000).to_be_bytes());
            bytes[0x9c] = 2;
            bytes[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
            bytes[0x5c..0x5e].copy_from_slice(&[2, 1]);
            let standard = record(&bytes)?;
            assert!(standard.unsupported_reason.is_none());
            assert!(standard.contact.survives_contact);
            assert_eq!(standard.contact.clashes, clashes);
            assert!(standard.motion.face_velocity);
            assert_eq!(
                standard.motion.response,
                ricochet.then_some(ProjectileResponse {
                    bounce,
                    ricochet,
                    ricochet_on_deflect: false,
                    ground_ricochet: false,
                    restitution: 0.75,
                })
            );
            bytes[8..12].copy_from_slice(&flags.to_be_bytes());
            assert_eq!(record(&bytes)?, standard);
            for (at, value) in [(0x8f, 255), (0x9c, 4), (0xb2, 1)] {
                let mut unsupported = bytes;
                unsupported[at] = value;
                assert!(record(&unsupported)?.unsupported_reason.is_some());
            }
        }
        let mut bytes = [0; 400];
        bytes[8..12].copy_from_slice(&0x1409_u32.to_be_bytes());
        bytes[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
        bytes[0x8c..0x90].copy_from_slice(&[128, 64, 64, 255]);
        let projectile = record(&bytes)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(
            projectile.motion.response,
            Some(ProjectileResponse {
                bounce: false,
                ricochet: true,
                ricochet_on_deflect: false,
                ground_ricochet: false,
                restitution: 0.75,
            })
        );
        bytes[0x18] = 1;
        assert!(record(&bytes)?.unsupported_reason.is_some());
        Ok(())
    }

    #[test]
    fn enabled_motion_and_effects_have_explicit_settings() -> Result<()> {
        let mut bytes = [0; 400];
        bytes[8..12].copy_from_slice(&0xc10048_u32.to_be_bytes());
        bytes[0x3c..0x40].copy_from_slice(&5_f32.to_be_bytes());
        bytes[0x94..0x98].copy_from_slice(&0.25_f32.to_be_bytes());
        bytes[0x98..0x9a].copy_from_slice(&[9, 2]);
        bytes[0x84..0x88].copy_from_slice(&[0, 3, 0, 4]);
        bytes[0x5e..0x60].copy_from_slice(&[2, 7]);
        bytes[0x90..0x93].copy_from_slice(&[0, 6, 4]);
        let projectile = record(&bytes)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.motion.speed, Some(5.));
        assert_eq!(
            projectile.motion.steering,
            Some(ProjectileSteering {
                blend: 0.25,
                start: 2,
                end: Some(9),
                planar: true,
            })
        );
        assert_eq!(projectile.active, Some([3, 7]));
        assert_eq!(projectile.trail_effect, Effect { bank: 2, member: 7 });
        assert_eq!(projectile.trail_interval, Some(6));
        assert!(projectile.shadow.unwrap().additive);
        assert!(projectile.clamp_ground && projectile.contact.survives_contact);

        for (lifetime, speed, birth, trail) in [(60_i16, 60_f32, 4, 0), (50, 50., 5, 6)] {
            let mut lance = [0; 400];
            lance[8..12].copy_from_slice(&0xd0609_u32.to_be_bytes());
            lance[0xc..0xe].copy_from_slice(&lifetime.to_be_bytes());
            lance[0x11] = 2;
            lance[0x14..0x18].copy_from_slice(&[1, 0, 1, 0]);
            for (at, value) in [(0x3c, speed), (0x40, 0.75), (0x44, 50.), (0x48, 50.)] {
                lance[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            lance[0x5c..0x60].copy_from_slice(&[0, birth, 2, trail]);
            lance[0x86..0x88].copy_from_slice(&10_i16.to_be_bytes());
            lance[0x88..0x90].copy_from_slice(&[96, 96, 96, 64, 128, 128, 128, 128]);
            if trail != 0 {
                lance[0x13] = 8;
                lance[0x90..0x93].copy_from_slice(&[0, 3, 4]);
            }
            lance[0x9c] = 4;
            let projectile = record(&lance)?;
            assert!(projectile.unsupported_reason.is_none());
            assert!(projectile.motion.aim_at_target && projectile.motion.stop_on_ground);
            assert_eq!(projectile.motion.speed, Some(speed));
            lance[0x18] = 1;
            assert!(record(&lance)?.unsupported_reason.is_some());
        }
        Ok(())
    }

    #[test]
    fn package_alignment_uses_absolute_offset_and_rejects_unexplained_tail() -> Result<()> {
        let mut bytes = vec![0; 400];
        bytes[8..12].copy_from_slice(&0x2009_u32.to_be_bytes());
        // The table starts at a word boundary; only the following member is
        // 32-byte aligned. Alignment bytes can contain unrelated data.
        bytes.extend(1_u8..=28);
        let table = read_package_member(&bytes, 0x7d4)?;
        assert_eq!(table.records.len(), 1);
        assert!(table.records[0].contact.survives_contact);
        assert!(table.records[0].contact.clashes);
        assert_eq!(table.source_sha256, crate::digest(&bytes));
        assert!(read(&bytes).is_err());
        assert!(read_package_member(&bytes, 0x7d0).is_err());
        assert!(read_package_member(&bytes[..427], 0x7d4).is_err());
        assert!(read_package_member(&bytes[..399], 0x7d4).is_err());
        assert!(read_package_member(&[], 0x7d4).is_err());
        assert!(read_package_member(&bytes[..400], 0x7d5).is_err());
        bytes.push(0);
        assert!(read_package_member(&bytes, 0x7d4).is_err());
        bytes.resize(460, 0);
        assert!(read_package_member(&bytes, 0x7d4).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires extracted game assets"]
    fn imports_lightning_projectile() -> Result<()> {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat");
        let bytes = std::fs::read(file)?;
        let table = read(crate::source_assets::section(&bytes, 7)?)?;
        assert_eq!(table.records.len(), 26);
        let lightning = &table.records[4];
        assert_eq!(lightning.lifetime, Some(20));
        assert_eq!(
            lightning.birth_effect,
            Effect {
                bank: 1,
                member: 28
            }
        );
        assert!(lightning.active.is_none());
        assert!(lightning.unsupported_reason.is_none());
        let spells = resident_spells(crate::source_assets::section(&bytes, 9)?)?;
        for (spell, power, recoil) in [(204, 60, 1), (208, 55, 6), (212, 65, 6), (216, 130, 1)] {
            let spell = &spells[&spell];
            assert_eq!(spell.duration, 90);
            assert_eq!(
                spell.hit.power,
                resonance_content::battle_action::Power::Percent(power)
            );
            assert_eq!(spell.hit.cooldown, 30);
            assert_eq!(spell.hit.recoil, recoil);
        }
        Ok(())
    }
}
