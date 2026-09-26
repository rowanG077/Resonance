//! Load the standard projectile template from the encounter's verified snapshot.
use super::{EffectResource, ProjectileResource, action};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    EffectAppearance, ProjectileContact, ProjectileDefinition, ProjectileEffects, ProjectileMotion,
    ProjectileShadow, ProjectileSteering,
};
use resonance_content::{battle_projectile::Table, prepared::Files, source::FloatOperand};

pub fn load(files: &Files, request: &ProjectileResource) -> Result<ProjectileDefinition> {
    let table: Table = files.json(&request.source)?;
    let row = table
        .records
        .get(usize::from(request.member))
        .with_context(|| {
            format!(
                "missing projectile {} in {}",
                request.member, request.source
            )
        })?;
    // 13B8C/14064/147E0: ballistic or direction-times-speed motion, with
    // optional target-snapshot steering. Specialized responses remain gated.
    ensure!(
        row.flags & !0xc1245f == 0,
        "projectile flags {:#x} are not prepared",
        row.flags
    );
    ensure!(
        row.velocity_reset_age == 0 && vector(row.velocity_jitter)? == [0.; 3],
        "projectile motion/effect controller is not prepared"
    );
    let kind = action::damage_kind(row.damage_kind)?;
    let hit = action::hit(files, &request.hit)?;
    let mut damage = action::damage(&hit, kind, row.hit_class != 0, &request.impact)?;
    damage.reaction =
        super::recoil::Parameters::load(files)?.reaction(&hit, row.reaction, row.knockback)?;
    ensure!(
        row.hit_class <= 1,
        "projectile contact response is not prepared"
    );
    let shape = action::shape(row.shape, row.inner_radius)?;
    let lifetime = u16::try_from(row.lifetime).context("invalid projectile lifetime")?;
    let active = if row.active_duration == 0 {
        None
    } else {
        ensure!(
            row.active_start >= 0 && row.active_duration > 0,
            "invalid projectile active interval"
        );
        let end = row
            .active_start
            .checked_add(row.active_duration)
            .context("projectile active interval overflow")?;
        Some([row.active_start as u16, end as u16])
    };
    let trail = effect(&request.trail, u16::from(row.trail_effect.member))?;
    if trail.is_some() {
        ensure!(
            row.pulse_state == 4 && row.trail_interval > 0,
            "projectile periodic effect controller is not prepared"
        );
    }
    Ok(ProjectileDefinition {
        lifetime,
        velocity: vector(row.velocity)?,
        acceleration: vector(row.acceleration)?,
        offset: vector(row.spawn_offset)?,
        clamp_ground: row.flags & 0x40 != 0,
        active,
        birth: effect(&request.birth, u16::from(row.birth_effect.member))?,
        motion: ProjectileMotion {
            speed: if row.flags & 0x10000 != 0 {
                let speed = row.speed.finite()?;
                ensure!(speed > 0., "invalid projectile speed");
                Some(speed)
            } else {
                None
            },
            steering: if row.flags & 0x400004 != 0 {
                let blend = row.steering_blend.finite()?;
                ensure!(
                    (0.0..=1.0).contains(&blend)
                        && (row.steering_end == 0 || row.steering_start < row.steering_end),
                    "invalid projectile steering"
                );
                Some(ProjectileSteering {
                    blend,
                    start: row.steering_start,
                    end: row.steering_end,
                    planar: row.flags & 0x400000 != 0,
                })
            } else {
                None
            },
        },
        effects: ProjectileEffects {
            trail: trail.map(|effect| (effect, row.trail_interval as u16)),
            ground: effect(&request.ground, u16::from(row.ground_effect.member))?,
            shadow: if row.flags & 0x400 == 0 {
                Some(ProjectileShadow {
                    color: row.shadow_color,
                    additive: row.flags & 0x800000 != 0,
                    radius: row.radius.finite()?,
                })
            } else {
                None
            },
        },
        contact: Some(ProjectileContact {
            hit: damage,
            cooldown: hit.contact_cooldown,
            repeat_limit: row.repeat_limit,
            radius: row.radius.finite()?,
            height: row.height.finite()?,
            shape,
            offset: vector(row.hit_offset)?,
            radius_growth: row.growth[0].finite()?,
            height_growth: row.growth[1].finite()?,
            survives_contact: row.flags & 8 != 0,
            clash_effect: effect(&request.clash, if row.flags & 0x2000 != 0 { 11 } else { 0 })?,
        }),
    })
}

fn vector(values: [FloatOperand; 3]) -> Result<[f32; 3]> {
    let [x, y, z] = values;
    Ok([x.finite()?, y.finite()?, z.finite()?])
}

fn effect(binding: &Option<EffectResource>, member: u16) -> Result<Option<EffectAppearance>> {
    if member == 0 {
        ensure!(binding.is_none(), "unexpected projectile effect binding");
        return Ok(None);
    }
    let binding = binding
        .as_ref()
        .context("missing projectile effect binding")?;
    ensure!(
        binding.members.contains(&member),
        "unbound projectile effect member {member}"
    );
    Ok(Some(EffectAppearance {
        resource: binding.resource,
        member,
    }))
}
