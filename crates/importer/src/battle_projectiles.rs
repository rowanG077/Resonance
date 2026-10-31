//! The resident battle projectile table directly into one editable document.
use crate::{dol, native_archive, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::{ActionHit, Condition, HitCondition, HitElement, Power},
    battle_projectile::{
        Contact, DamageKind, Effect, FireBall, HitShape, Projectile, ProjectileDeparture,
        ProjectileMotion, ProjectileResponse, ProjectileShadow, ProjectileSteering,
        ProjectileVelocityReset, RecoilDirection, ResidentSpell, Table, VERSION,
    },
    menu_data::Element,
};
use std::{collections::BTreeMap, fs, path::Path};

fn float(data: &[u8], at: usize) -> Result<f32> {
    let value = f32::from_bits(dol::u32(data, at)?);
    ensure!(value.is_finite(), "non-finite projectile value at {at:#x}");
    Ok(value)
}

fn vector<const N: usize>(data: &[u8], at: usize) -> Result<[f32; N]> {
    (0..N)
        .map(|index| float(data, at + index * 4))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid projectile vector"))
}

fn effect(row: &[u8], at: usize) -> Effect {
    Effect {
        bank: row[at],
        member: row[at + 1],
    }
}

/// Decode one native 400-byte projectile row shared by resident spells and
/// stored battle scenes. The caller supplies the REL's acceleration reset scale.
pub(crate) fn scene_record(row: &[u8], reset_scale: f32) -> Result<Projectile> {
    ensure!(row.len() == 400, "invalid projectile record size");
    ensure!(
        [0..8, 0x10..0x11, 0x18..0x24, 0x58..0x5c, 0x9e..400]
            .into_iter()
            .all(|range| row[range].iter().all(|&byte| byte == 0)),
        "projectile has unsupported instance state"
    );
    let flags = dol::u32(row, 8)? & !0x10; // Native standard declaration bit.
    let ground_lance = flags == 0xd0609 && row[0x9c] == 4 && row[0x14..0x18] == [1, 0, 1, 0];
    let departure = flags == 0x0200_1049 && row[0x9c] == 0 && row[0x14..0x18] == [1, 0, 0, 0];
    let clashing_flight = flags == 0x620a && row[0x9c] == 2 && row[0x14] == 1;
    ensure!(
        // 0x4000 allocates native point history; authored birth/trail effects
        // already carry the editable visual and the history is never submitted.
        // Enemy-local records use the same clash, steering and ricochet bits
        // that the direct fields below already represent.
        flags & !(0x1f9_367f | 0x4000) == 0 || ground_lance || departure || clashing_flight,
        "unsupported projectile controller {flags:#x}"
    );
    ensure!(
        matches!(row[0x9c], 0 | 2 | 4) && row[0x9a..0x9c] == [0; 2],
        "unsupported projectile motion mode"
    );
    let active_start = dol::u16(row, 0x84)? as i16;
    let active_duration = dol::u16(row, 0x86)? as i16;
    ensure!(
        active_start >= 0 && active_duration >= 0,
        "invalid projectile active interval"
    );
    let trail = effect(row, 0x5e);
    let trail_interval = if trail.member != 0 {
        ensure!(row[0x92] == 4, "unsupported periodic effect mode");
        let interval = dol::u16(row, 0x90)? as i16;
        ensure!(interval > 0, "invalid periodic effect interval");
        Some(interval as u32)
    } else {
        None
    };
    let lifetime = dol::u16(row, 0xc)? as i16;
    ensure!(lifetime >= 0, "invalid projectile lifetime");
    let radius = float(row, 0x44)?;
    let shape = match row[0x15] {
        0 => HitShape::Box,
        1 => HitShape::Cylinder,
        3 => HitShape::Ring {
            width: float(row, 0x4c)?,
        },
        4 => HitShape::Sphere,
        kind => anyhow::bail!("unsupported projectile contact shape {kind}"),
    };
    let response = (flags & 0x101020 != 0 && !departure)
        .then(|| -> Result<_> {
            Ok(ProjectileResponse {
                bounce: flags & 0x20 != 0,
                ricochet: flags & 0x1000 != 0,
                ricochet_on_deflect: flags & 0x100000 != 0,
                ground_ricochet: false,
                restitution: float(row, 0x40)?,
            })
        })
        .transpose()?;
    let steering = (flags & 0x400004 != 0)
        .then(|| -> Result<_> {
            Ok(ProjectileSteering {
                blend: float(row, 0x94)?,
                start: u32::from(row[0x99]),
                end: (row[0x98] != 0).then_some(u32::from(row[0x98])),
                planar: flags & 0x400000 != 0,
            })
        })
        .transpose()?;
    let velocity_reset = (row[0x9d] != 0).then_some(ProjectileVelocityReset {
        age: u32::from(row[0x9d]),
        acceleration_scale: reset_scale,
    });
    let recoil_direction = match row[0x16] {
        0 => RecoilDirection::Travel,
        1 => RecoilDirection::AwayFromOwner,
        2 => RecoilDirection::AwayFromContact,
        kind => anyhow::bail!("unsupported projectile recoil direction {kind}"),
    };
    Ok(Projectile {
        lifetime: (lifetime > 0).then_some(lifetime as u32),
        velocity: vector(row, 0x24)?,
        acceleration: vector(row, 0x30)?,
        spawn_offset: vector(row, 0x60)?,
        clamp_ground: flags & 0x40 != 0,
        active: (active_duration > 0)
            .then_some([active_start as u32, (active_start + active_duration) as u32]),
        motion: ProjectileMotion {
            departure: departure.then_some(ProjectileDeparture {
                return_speed: 4.,
                jump_speed: 10.,
                ground_speed: 12.,
                ground_reference: [1., 0.],
            }),
            velocity_jitter: vector(row, 0x6c)?,
            speed: (flags & 0x10000 != 0)
                .then(|| float(row, 0x3c))
                .transpose()?,
            steering,
            response,
            velocity_reset,
            aim_at_target: ground_lance,
            face_velocity: flags & 0x200 != 0,
            world_direction: flags & 0x200000 != 0,
            stop_on_ground: flags & 0x80000 != 0,
            unlimited_range: flags & 0x1000000 != 0,
            ..ProjectileMotion::default()
        },
        contact: Contact {
            repeat_limit: row[0x17],
            shape,
            recoil_direction,
            radius,
            height: float(row, 0x48)?,
            growth: vector(row, 0x50)?,
            offset: vector(row, 0x78)?,
            survives_contact: flags & 8 != 0,
            clashes: flags & 0x2000 != 0,
        },
        birth_effect: effect(row, 0x5c),
        ground_effect: effect(row, 0xe),
        trail_effect: trail,
        trail_interval,
        shadow: (flags & 0x400 == 0).then_some(ProjectileShadow {
            color: row[0x88..0x8c].try_into()?,
            additive: flags & 0x800000 != 0,
            radius,
        }),
        unsupported_reason: None,
    })
}

/// Decode the simple contact rule used by resident and stored spell scenes.
/// The native effect-bank selector has no meaning when no effect is selected.
pub(crate) fn scene_hit(row: &[u8], recoil: u8) -> Result<ActionHit> {
    ensure!(row.len() >= 28, "truncated resident spell hit");
    let flags = dol::u16(row, 0)?;
    let condition = dol::u32(row, 8)?;
    ensure!(
        flags & !0x622f == 0 && row[13] == 1 && matches!(condition, 0 | 0x200),
        "unsupported resident spell hit"
    );
    ensure!(
        row[23] == 0 && row[25..28] == [0; 3] && row[18..21] == [0; 3] && matches!(row[24], 0 | 6),
        "unsupported resident spell effect"
    );
    let element = match row[2] {
        0 => HitElement::Inherited,
        10 => HitElement::Neutral,
        value @ 1..=8 => HitElement::Element(Element::ALL[usize::from(value - 1)]),
        _ => anyhow::bail!("invalid spell element {}", row[2]),
    };
    Ok(ActionHit {
        kind: DamageKind::Magic,
        effect: (row[22] != 0).then_some(u16::from(row[22])),
        effect_on_guard: false,
        suppress_guard_sound: false,
        capture: false,
        sound: (dol::u16(row, 16)? != 0).then_some(dol::u16(row, 16)?),
        power: Power::Percent(dol::u16(row, 14)?),
        element,
        arte: true,
        prevents_defeat: flags & 1 != 0,
        finisher_grade: flags & 0x200 != 0,
        hp_drain: false,
        suppress_reaction: flags & 0x4000 != 0,
        force_reaction: flags & 0x2000 != 0,
        alternate_motion: flags & 2 != 0,
        knock_down: flags & 4 != 0,
        launch: flags & 8 != 0,
        hits_down: false,
        breaks_guard: false,
        unbreakable_guard: false,
        lift_guard: false,
        guard_pressure: row[7],
        bypass_guard: false,
        hitstun: row[3],
        cooldown: row[4],
        stun_chance: row[5],
        stagger: row[6],
        armor_damage: row[20],
        recoil,
        recoil_delay: row[21],
        condition: (condition == 0x200).then_some(HitCondition {
            condition: Condition::Heavy,
            chance: row[12],
            value: row[23] as i8,
        }),
    })
}

fn spell(parameters: &[u8], id: u16) -> Result<&[u8]> {
    let index = usize::from(id - 200);
    ensure!(
        index < dol::u32(parameters, 0)? as usize,
        "missing resident spell {id}"
    );
    let start = dol::u32(parameters, 4 + index * 4)? as usize;
    parameters
        .get(start..)
        .context("spell parameters exceed table")
}

fn convert(root: &Path) -> Result<Table> {
    let disc = root.join("disc1/files");
    let usual = fs::read(disc.join("BTL/BTLusual.dat"))?;
    let rows =
        native_archive::member(&usual, 7, 14)?.context("resident projectile table is missing")?;
    ensure!(
        rows.len() == 26 * 400,
        "unexpected resident projectile count"
    );
    let parameters =
        native_archive::member(&usual, 9, 14)?.context("spell parameters are missing")?;
    let module = Rel::read(fs::read(disc.join("US_r_Top2Btl.rel"))?)?;
    let reset_scale = float(module.slice(4, 0xce8, 4)?, 0)?;
    let records = rows
        .chunks_exact(400)
        .map(|row| scene_record(row, reset_scale))
        .collect::<Result<_>>()?;
    let mut resident_spells = BTreeMap::new();
    for (id, recoil) in [(200, 6), (204, 1), (208, 6), (212, 6), (216, 1), (220, 10)] {
        let source = spell(parameters, id)?;
        let start = dol::u32(source, 0)? as usize;
        let end = dol::u32(source, 4)? as usize;
        let rules = source
            .get(start..end)
            .context("spell hit rules exceed parameters")?;
        resident_spells.insert(
            id,
            ResidentSpell {
                duration: dol::u16(source, 16)?,
                hit: scene_hit(rules.get(..28).context("spell has no hit")?, recoil)?,
                followup_hit: (id == 220)
                    .then(|| scene_hit(rules.get(28..56).context("spell has no followup hit")?, 7))
                    .transpose()?,
            },
        );
    }
    let mut support_durations = BTreeMap::new();
    for id in [239, 242, 245, 248, 254, 273, 274, 275] {
        let duration = dol::u16(spell(parameters, id)?, 16)?;
        ensure!(duration > 0, "empty support spell duration");
        support_durations.insert(id, duration);
    }
    let fire = module.slice(4, 0x3928, 44)?;
    Ok(Table {
        version: VERSION,
        records,
        resident_spells,
        support_durations,
        fire_ball: Some(FireBall {
            velocities: [vector(fire, 0)?, vector(fire, 12)?, vector(fire, 24)?],
            height_offset: float(fire, 36)?,
            center_height_scale: float(fire, 40)?,
        }),
    })
}

pub fn cook_battle_projectiles(root: &Path, library: &Path) -> Result<()> {
    let table = convert(root)?;
    let output = library.join("battle/projectiles.json");
    crate::output::json(&output, &table)?;
    Ok(())
}
