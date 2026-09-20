//! Recover projectile recipes; pointer fields never enter the cooked catalogue.
pub(super) mod all;
mod binding;
pub(crate) use binding::{bind, bind_one};
#[cfg(test)]
#[path = "effects/explosion_tests.rs"]
mod explosion_tests;
#[cfg(test)]
#[path = "effects/guardian_tests.rs"]
mod guardian_tests;
mod source;

use super::effect_program::{MagicArchive, magic_member};
#[cfg(test)]
use crate::compression;
use crate::read::{f32 as float, u16 as half, u32 as word};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::battle::{
    actions::{HitShape, HitShapeKind},
    effects::*,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[cfg(test)]
pub(crate) fn cook(extracted: &Path, required: &BTreeSet<EffectId>) -> Result<BattleEffects> {
    let sources = super::all::Sources::read(extracted)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let recipes = super::actions::member(&usual, 7)?;
    let rel = super::actions::Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
    let velocity_reset_scale = super::motion::read(&rel, &super::embedded::Layout::RETAIL)?
        .projectile_velocity_reset_scale()?;
    let table = word(&usual, 0x2c)? as usize;
    let archive = fs::read(extracted.join("files").join(&sources.enemy))?;
    let mut enemies = BTreeMap::new();
    let magic = required
        .iter()
        .any(|id| matches!(id.bank, EffectBank::Magic(_)))
        .then(|| MagicArchive::read(extracted))
        .transpose()?;
    let projectiles = required
        .iter()
        .map(|&id| {
            let recipes = match id.bank {
                EffectBank::Techniques => recipes,
                EffectBank::Enemy(monster) => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        enemies.entry(monster)
                    {
                        let start = word(&usual, table + usize::from(monster) * 4)? as usize;
                        let end = word(&usual, table + (usize::from(monster) + 1) * 4)? as usize;
                        entry.insert(compression::decode(
                            archive
                                .get(start..end)
                                .context("missing enemy projectile package")?,
                        )?);
                    }
                    let enemy = &enemies[&monster];
                    super::enemy_inventory::offset_section(enemy, word(enemy, 0x1c8)? as usize)?
                }
                EffectBank::Common => {
                    bail!("common projectile recipes require a recovered source binding")
                }
                EffectBank::Skill(_) | EffectBank::Arena(_) => {
                    bail!("projectile recipes for this resource bank are not implemented")
                }
                EffectBank::Magic(package) => magic_member(
                    magic
                        .as_ref()
                        .context("missing magic archive")?
                        .package(package)?,
                    252,
                )?
                .with_context(|| format!("missing magic {package} projectile recipes"))?,
            };
            projectile(
                recipes
                    .get(usize::from(id.id) * 400..(usize::from(id.id) + 1) * 400)
                    .with_context(|| format!("missing selected projectile {id:?}"))?,
                id,
                velocity_reset_scale,
            )
            .with_context(|| format!("projectile {id:?}"))
        })
        .collect::<Vec<_>>();
    collect(projectiles)
}

fn collect(projectiles: Vec<Result<ProjectileRecipe>>) -> Result<BattleEffects> {
    let failures = projectiles
        .iter()
        .filter_map(|result| result.as_ref().err())
        .map(|error| format!("{error:#}"))
        .collect::<Vec<_>>();
    ensure!(
        failures.is_empty(),
        "projectile preflight failed:\n{}",
        failures.join("\n")
    );
    let projectiles = projectiles.into_iter().collect::<Result<_>>()?;
    let effects = BattleEffects { projectiles };
    effects.validate()?;
    Ok(effects)
}

// Legacy point trails are allocated but their object mode is excluded from draw submission.
const UNSUBMITTED_LINE: u32 = 0x4000;
const CONTACT_DEBRIS: u32 = 0x1000 | 0x100000;

#[cfg(test)]
pub(super) fn projectile(
    row: &[u8],
    id: EffectId,
    velocity_reset_scale: f32,
) -> Result<ProjectileRecipe> {
    lower(
        &source::AuthoredProjectile::decode(row)?,
        id,
        velocity_reset_scale,
    )
}

fn lower(
    source: &source::AuthoredProjectile,
    id: EffectId,
    velocity_reset_scale: f32,
) -> Result<ProjectileRecipe> {
    let flags = source.flags;
    let birth_trail_bank = native_birth_bank(id.bank);
    // Model-bone contact origins and remaining emitter flags need separate bindings.
    const SUPPORTED: u32 = 0x1
        // Retained in authored rows; the standard init/update/retirement ignore this bit.
        | 0x2
        | 0x4
        | 0x8
        | 0x10
        | 0x20
        | 0x40
        | 0x80
        | 0x200
        | 0x400
        | 0x1000
        | 0x2000
        | UNSUBMITTED_LINE
        | 0x8000
        | 0x10000
        | 0x20000
        | 0x40000
        | 0x80000
        | 0x100000
        | 0x200000
        | 0x400000
        | 0x800000
        | 0x1000000
        | 0x2000000;
    ensure!(
        flags & !SUPPORTED == 0,
        "unsupported projectile movement flags {:#x}",
        flags & !SUPPORTED
    );
    ensure!(
        !(flags & 0x8000 != 0 || (flags & CONTACT_DEBRIS != 0 && flags & 0x2000000 == 0))
            || flags & 0x10000 == 0,
        "projectile scattering requires ballistic motion"
    );
    ensure!(
        source.pulse_state & !4 == 0,
        "unsupported projectile periodic pulse state"
    );
    ensure!(
        source.velocity_reset_age == 0 || flags & 0x10000 == 0,
        "directed projectile reset requires retained acceleration"
    );
    let bank = |value| -> Result<EffectBank> {
        match value {
            0 => Ok(EffectBank::Common),
            1 => Ok(EffectBank::Techniques),
            2 => match id.bank {
                EffectBank::Enemy(monster) => Ok(EffectBank::Enemy(monster)),
                _ => bail!("enemy effect bank on party projectile"),
            },
            6 => match id.bank {
                EffectBank::Magic(package) => Ok(EffectBank::Magic(package)),
                EffectBank::Skill(package) => Ok(EffectBank::Skill(package)),
                _ => bail!("dynamic effect bank on a projectile without a dynamic package"),
            },
            other => bail!("unsupported projectile effect bank {other}"),
        }
    };
    let effect = |selector: source::EffectSelector,
                  override_bank: Option<EffectBank>|
     -> Result<Option<EffectId>> {
        if selector.id == 0 {
            return Ok(None);
        }
        Ok(Some(EffectId {
            bank: override_bank.map_or_else(|| bank(selector.slot), Ok)?,
            id: selector.id,
        }))
    };
    let steering = (flags & 0x400004 != 0).then_some(ProjectileSteering {
        blend: source.steering_blend,
        start: source.steering_start,
        end: (source.steering_end != 0).then_some(source.steering_end),
        horizontal: flags & 0x400000 != 0,
    });
    let movement = match (flags & 0x10000 != 0, steering) {
        (true, Some(steering)) => ProjectileMovement::Homing {
            direction: source.velocity,
            speed: source.speed,
            blend: steering.blend,
            start: steering.start,
            end: steering.end,
            horizontal: steering.horizontal,
        },
        (true, None) => ProjectileMovement::Directed {
            direction: source.velocity,
            speed: source.speed,
        },
        (false, steering) => ProjectileMovement::Ballistic {
            velocity: source.velocity,
            acceleration: source.acceleration,
            steering,
        },
    };
    let trail_effect = if source.pulse_state & 4 != 0 {
        effect(source.trail_effect, birth_trail_bank)?
    } else {
        None
    };
    ensure!(
        trail_effect.is_none() || source.trail_interval > 0,
        "zero projectile trail interval"
    );
    Ok(ProjectileRecipe {
        id: Some(id),
        lifetime: source.lifetime,
        movement,
        behavior: ProjectileBehavior {
            aim: if flags & 0x40000 != 0 {
                ProjectileAim::Target
            } else if flags & 0x20000 != 0 {
                ProjectileAim::Bones {
                    toward: source.toward_bone,
                    from: source.from_bone,
                }
            } else if flags & 0x200000 != 0 {
                ProjectileAim::World
            } else {
                ProjectileAim::Heading
            },
            spawn_at_target: flags & 0x80 != 0,
            face_velocity: flags & 0x200 != 0,
            clamp_ground: flags & 0x40 != 0,
            stop_on_ground: flags & 0x80000 != 0,
            scatter_on_ground: flags & 0x8000 != 0,
            bounce_restitution: (flags & (CONTACT_DEBRIS | 0x20) != 0)
                .then_some(source.bounce_restitution),
            bounce_after_contact: flags & CONTACT_DEBRIS != 0 && flags & 0x20 == 0,
            contact_response: match (
                flags & 0x1000 != 0,
                flags & 0x100000 != 0,
                flags & 0x2000000 != 0,
            ) {
                (true, _, false) => ProjectileContactResponse::BounceAway,
                (true, _, true) => ProjectileContactResponse::Disarm,
                (false, true, false) => ProjectileContactResponse::BounceAwayOnBlock,
                (false, true, true) => ProjectileContactResponse::DisarmOnBlock,
                _ => ProjectileContactResponse::Ordinary,
            },
            unlimited_range: flags & 0x1000000 != 0,
            repeat_limit: source.repeat_limit,
            hit_growth: source.hit_growth,
            velocity_reset: std::num::NonZeroU8::new(source.velocity_reset_age).map(|age| {
                ProjectileVelocityReset {
                    age,
                    acceleration_scale: velocity_reset_scale,
                }
            }),
        },
        velocity_jitter: source.velocity_jitter,
        spawn_offset: source.spawn_offset,
        hit_offset: source.hit_offset,
        shape: source.shape,
        knockback: source.knockback,
        active: (source.active_duration != 0)
            .then(|| {
                Ok::<_, anyhow::Error>([
                    source.active_start,
                    source
                        .active_start
                        .checked_add(source.active_duration)
                        .context("projectile contact range overflow")?,
                ])
            })
            .transpose()?,
        persist_after_hit: flags & 8 != 0,
        clashable: flags & 0x2000 != 0,
        // Standard initialization reads the bank only when the birth ID is nonzero.
        // Some party rows retain the enemy selector in their disabled sequence.
        birth_bank: if source.spawn_effect.slot == 2
            && source.spawn_effect.id == 0
            && id.bank == EffectBank::Techniques
        {
            None
        } else {
            Some(birth_trail_bank.map_or_else(|| bank(source.spawn_effect.slot), Ok)?)
        },
        spawn_effect: effect(source.spawn_effect, birth_trail_bank)?,
        trail_effect,
        trail_interval: source.trail_interval.max(1),
        ground_effect: effect(source.ground_effect, None)?,
        shadow: (flags & 0x400 == 0).then_some(ProjectileShadow {
            color: source.shadow_color,
            radius: source.shape.radius,
            additive: flags & 0x800000 != 0,
        }),
    })
}

// Allocator specialization belongs to selected battle controllers. Exhaustive
// source conversion retains authored banks before those allocators overwrite them.
fn native_birth_bank(bank: EffectBank) -> Option<EffectBank> {
    matches!(
        bank,
        EffectBank::Magic(
            1 | 2
                | 3
                | 5
                | 6
                | 7
                | 9
                | 10
                | 11
                | 13
                | 14
                | 15
                | 17
                | 18
                | 19
                | 21
                | 22
                | 23
                | 24
                | 26
                | 27
                | 28
                | 29
                | 30
                | 31
                | 32
                | 33
                | 51
                | 52
                | 53
                | 78
                | 83
                | 84
                | 85
                | 86
                | 87
                | 88
                | 89
                | 90
                | 91
                | 92
                | 93
                | 105
                | 107
                | 108
                | 109
                | 110
                | 111
                | 113
                | 115
                | 116
                | 117
                | 118
        )
    )
    .then_some(bank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_devastation_has_no_birth_binding_until_an_effect_is_requested() {
        // Complete original projectile17 fields consumed by the standard controller.
        let encoded = "000000000000000000000009001800000000010100010104000000000000000000000000000000000000000000000000000000000000000000000000000000003f40000042c00000434800000000000040c000000000000000000000020002000000000042c800004248000000000000000000000000000000000000000000000000000000000000181818900000000000000000000000000000000002000000";
        let mut row = [0; 400];
        for (i, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
            row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let id = EffectId {
            bank: EffectBank::Techniques,
            id: 17,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        assert!(
            recipe.birth_bank.is_none()
                && recipe.spawn_effect.is_none()
                && recipe.trail_effect.is_none()
        );
        assert_eq!(
            (
                recipe.lifetime,
                recipe.shape.radius,
                recipe.shape.height,
                recipe.behavior.repeat_limit
            ),
            (24, 96., 200., 4)
        );
        assert_eq!(recipe.behavior.hit_growth, [6., 0.]);
        assert_eq!(recipe.spawn_offset, [0., 100., 50.]);
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 0., 0.],
                acceleration: [0., 0., 0.],
                steering: None
            }
        ));
        assert!(recipe.persist_after_hit && recipe.shadow.is_some());
        row[0x5d] = 1;
        assert!(projectile(&row, id, 0.001).is_err());
        row[0x5d] = 0;
        row[0x5f] = 1;
        row[0x92] = 4;
        row[0x91] = 1;
        assert!(projectile(&row, id, 0.001).is_err());
        row[0x5f] = 0;
        row[0x5c] = 255;
        assert!(projectile(&row, id, 0.001).is_err());
    }

    #[test]
    fn spread_resolves_stored_pool_banks_without_rewriting_ground_effects() {
        let bytes = concat!(
            "000000000000000000000409001e000000020009010102040000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "3f400000432a000043c800000000000000000000000000000000000002000200",
            "0000000000000000000000000000000000000000000000000000000043480000",
            "0000000000000000000000800000000000000000000000000000000000000000",
        );
        let mut row = [0; 400];
        for (i, pair) in bytes.as_bytes().chunks_exact(2).enumerate() {
            row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let id = EffectId {
            bank: EffectBank::Magic(1),
            id: 1,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert_eq!(recipe.birth_bank, Some(id.bank));
        assert_eq!(
            (recipe.lifetime, recipe.shape.radius, recipe.shape.height),
            (30, 170., 400.)
        );
        assert_eq!(recipe.hit_offset, [0., 200., 0.]);
        assert_eq!(recipe.behavior.repeat_limit, 4);
        assert!(recipe.persist_after_hit && recipe.shadow.is_none());

        // Enabling an authored birth or trail keeps this package's identity;
        // a ground bank is never silently redirected into a resident slot.
        row[0x5d] = 1;
        row[0x5f] = 1;
        row[0x91] = 1;
        row[0x92] = 4;
        let recipe = projectile(&row, id, 0.001).unwrap();
        assert_eq!(recipe.spawn_effect, Some(id));
        assert_eq!(recipe.trail_effect, Some(id));
        row[0xe] = 2;
        row[0xf] = 1;
        assert!(projectile(&row, id, 0.001).is_err());
        row[0xf] = 0;
        assert!(
            projectile(
                &row,
                EffectId {
                    bank: EffectBank::Magic(4),
                    ..id
                },
                0.001
            )
            .is_err()
        );
    }

    #[test]
    fn aqua_edge_keeps_script_owned_lifetime_and_both_water_visuals() {
        let encoded = "00000000000000000181000b000001000002000601000100000000000000000000000000000000000000000000000000000000000000000000000000417000003f400000428c00004220000000000000000000000000000000000000012b012c00000000420c00000000000000000000000000000000000000000000000000000000000000000000202060400000000000020400424800000000000000000000";
        let mut row = [0; 400];
        for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
            row[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let recipe = projectile(
            &row,
            EffectId {
                bank: EffectBank::Techniques,
                id: 8,
            },
            0.001,
        )
        .unwrap();
        recipe.validate().unwrap();
        assert_eq!(recipe.lifetime, 0);
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Directed {
                direction: [0., 0., 0.],
                speed: 15.
            }
        ));
        assert_eq!(recipe.spawn_offset, [0., 35., 0.]);
        assert_eq!(
            (
                recipe.spawn_effect.unwrap().id,
                recipe.trail_effect.unwrap().id,
                recipe.trail_interval
            ),
            (43, 44, 2)
        );
        assert!(
            recipe.persist_after_hit
                && recipe.behavior.unlimited_range
                && recipe.shadow.unwrap().additive
        );
        assert_eq!((recipe.shape.radius, recipe.shape.height), (70., 40.));
    }

    #[test]
    fn lightning_strike_recovers_stationary_volume_and_ignores_unused_authoring_bit() {
        let bytes = concat!(
            "00000000000000000000044a0014000000020001010101000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "3f4000004220000043c8000000000000000000000000000000000000011c0100",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000040000000000800000000000000000000000000000000000000000",
        );
        let mut row = [0; 400];
        for (i, pair) in bytes.as_bytes().chunks_exact(2).enumerate() {
            row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let id = EffectId {
            bank: EffectBank::Techniques,
            id: 4,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert_eq!((recipe.lifetime, recipe.active), (20, None));
        assert_eq!((recipe.shape.radius, recipe.shape.height), (40., 400.));
        assert_eq!((recipe.shape.damage_kind, recipe.shape.hit_class), (2, 0));
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 0., 0.],
                acceleration: [0., 0., 0.],
                steering: None
            }
        ));
        assert_eq!(recipe.spawn_effect.unwrap().id, 28);
        assert!(
            recipe.ground_effect.is_none()
                && recipe.trail_effect.is_none()
                && recipe.shadow.is_none()
        );
        assert!(recipe.behavior.clamp_ground && recipe.persist_after_hit && !recipe.clashable);
        row[8..12].copy_from_slice(&0x44a_u32.wrapping_add(0x800).to_be_bytes());
        assert!(
            projectile(&row, id, 0.001).is_err(),
            "bone volumes still require their own implementation"
        );
    }

    #[test]
    fn satellite_reset_uses_its_age_byte_and_rejects_unretained_directed_acceleration() {
        let mut row = [0; 400];
        row[8..12].copy_from_slice(&9_u32.to_be_bytes());
        row[12..14].copy_from_slice(&45_u16.to_be_bytes());
        row[0x2c..0x30].copy_from_slice(&20_f32.to_be_bytes());
        row[0x9c] = 2;
        row[0x9d] = 6;
        let id = EffectId {
            bank: EffectBank::Techniques,
            id: 11,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        let reset = recipe.behavior.velocity_reset.unwrap();
        assert_eq!(reset.age.get(), 6);
        assert_eq!(reset.acceleration_scale, 0.001);
        assert_eq!(recipe.lifetime, 45);
        row[0x9d] = 0;
        assert!(
            projectile(&row, id, 0.001)
                .unwrap()
                .behavior
                .velocity_reset
                .is_none()
        );
        row[0x9d] = 6;
        row[8..12].copy_from_slice(&0x10009_u32.to_be_bytes());
        assert!(projectile(&row, id, 0.001).is_err());
    }

    #[test]
    fn hammer_contact_responses_preserve_authored_motion() {
        // Original BTLusual projectile 7, through the last authored byte; runtime tail is zero.
        let bytes = concat!(
            "0000000000000000000010290050011200000101000001000000000000000000",
            "0000000000000000415000004090000000000000bf0ccccd0000000000000000",
            "3f40000041f0000041f0000000000000000000000000000000000000010f0100",
            "41a0000042b40000428200000000000000000000000000000000000042200000",
            "000000000000004600000080804040ff00000000000000000000000004000000"
        );
        let mut row = [0; 400];
        for (i, pair) in bytes.as_bytes().chunks_exact(2).enumerate() {
            row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let id = EffectId {
            bank: EffectBank::Techniques,
            id: 7,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert_eq!(
            recipe.behavior.contact_response,
            ProjectileContactResponse::BounceAway
        );
        assert_eq!(recipe.behavior.bounce_restitution, Some(0.75));
        assert!(!recipe.behavior.bounce_after_contact);
        assert!(recipe.persist_after_hit && !recipe.clashable);
        assert_eq!((recipe.lifetime, recipe.active), (80, Some([0, 70])));
        assert_eq!(
            (
                recipe.spawn_effect.unwrap().id,
                recipe.ground_effect.unwrap().id
            ),
            (15, 18)
        );
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 13., 4.5],
                acceleration: [0., -0.55, 0.],
                steering: None
            }
        ));
        for (flags, response, scatter) in [
            (0x101029_u32, ProjectileContactResponse::BounceAway, false),
            (
                0x100029,
                ProjectileContactResponse::BounceAwayOnBlock,
                false,
            ),
            (0x9029, ProjectileContactResponse::BounceAway, true),
            (0x2001029, ProjectileContactResponse::Disarm, false),
            (0x2100029, ProjectileContactResponse::DisarmOnBlock, false),
            (0x2011029, ProjectileContactResponse::Disarm, false), // Directed motion stays intact.
        ] {
            row[8..12].copy_from_slice(&flags.to_be_bytes());
            let recipe = projectile(&row, id, 0.001).unwrap();
            recipe.validate().unwrap();
            assert_eq!(recipe.behavior.contact_response, response);
            assert_eq!(recipe.behavior.scatter_on_ground, scatter);
        }
        for flags in [0x11029_u32, 0x110029, 0x19029] {
            row[8..12].copy_from_slice(&flags.to_be_bytes());
            assert!(
                projectile(&row, id, 0.001).is_err(),
                "unimplemented debris mode {flags:#x}"
            );
        }
    }

    #[test]
    fn pinion12_recovers_and_validates_delayed_ground_bounce() {
        // Complete original projectile12 at BTLusual+d48e0; every remaining byte is zero.
        let bytes = concat!(
            "0000000000000000000014090024000000000101000001000000000000000000",
            "0000000000000000000000004140000000000000000000000000000000000000",
            "3f40000041a00000420c000000000000000000000000000000000000013e0100",
            "0000000042dc0000424800000000000000000000000000000000000000000000",
            "000000000000002000000080804040ff"
        );
        let mut row = [0; 400];
        for (i, pair) in bytes.as_bytes().chunks_exact(2).enumerate() {
            row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let id = EffectId {
            bank: EffectBank::Techniques,
            id: 12,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert!(recipe.behavior.bounce_after_contact);
        assert_eq!(recipe.behavior.bounce_restitution, Some(0.75));
        assert_eq!(
            recipe.behavior.contact_response,
            ProjectileContactResponse::BounceAway
        );
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 0., 12.],
                acceleration: [0., 0., 0.],
                steering: None
            }
        ));
        assert_eq!((recipe.lifetime, recipe.active), (36, Some([0, 32])));
        assert_eq!(
            (
                recipe.spawn_offset,
                recipe.hit_offset,
                recipe.velocity_jitter
            ),
            ([0., 110., 50.], [0.; 3], [0.; 3])
        );
        assert_eq!(
            (
                recipe.shape.radius,
                recipe.shape.height,
                recipe.shape.inner_radius
            ),
            (20., 35., 0.)
        );
        assert_eq!(
            recipe.spawn_effect,
            Some(EffectId {
                bank: EffectBank::Techniques,
                id: 62
            })
        );
        assert!(recipe.persist_after_hit && !recipe.clashable && recipe.shadow.is_none());
        assert!(recipe.ground_effect.is_none() && recipe.trail_effect.is_none());
        let mut invalid = recipe.clone();
        invalid.behavior.contact_response = ProjectileContactResponse::Ordinary;
        assert!(invalid.validate().is_err());
        invalid = recipe;
        invalid.behavior.bounce_restitution = None;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn projectile_policies_are_decoded_without_accepting_missing_controllers() {
        let mut row = [0; 400];
        let id = EffectId {
            bank: EffectBank::Enemy(51),
            id: 0,
        };
        // Enemy 51's simple projectile uses 0x11; flags are independent policies,
        // not an allowlist of entire monster-specific flag combinations.
        row[8..12].copy_from_slice(&0x11_u32.to_be_bytes());
        // The multiplier alone cannot enable bouncing.
        row[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert_eq!(recipe.behavior.bounce_restitution, None);
        // Enemy 0/1 projectile 0 combines bounce, clamp, and opposing-volume clashes.
        row[8..12].copy_from_slice(&0x2061_u32.to_be_bytes());
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert_eq!(recipe.behavior.bounce_restitution, Some(0.75));
        assert!(recipe.behavior.clamp_ground && recipe.clashable);
        let flags = 0x400004_u32 | 0x200000 | 0x60 | 0x80 | 0x1000000;
        row[8..12].copy_from_slice(&flags.to_be_bytes());
        row[0x17] = 2;
        row[0x50..0x54].copy_from_slice(&3_f32.to_be_bytes());
        row[0x94..0x98].copy_from_slice(&0.25_f32.to_be_bytes());
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert!(matches!(recipe.behavior.aim, ProjectileAim::World));
        assert!(
            recipe.behavior.spawn_at_target
                && recipe.behavior.clamp_ground
                && recipe.behavior.unlimited_range
        );
        assert_eq!(recipe.behavior.repeat_limit, 2);
        assert_eq!(recipe.behavior.hit_growth, [3., 0.]);
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Ballistic {
                steering: Some(ProjectileSteering {
                    horizontal: true,
                    blend: 0.25,
                    ..
                }),
                ..
            }
        ));
        for missing in [0x800_u32, 0x4000000] {
            row[8..12].copy_from_slice(&(flags | missing).to_be_bytes());
            assert!(
                projectile(&row, id, 0.001).is_err(),
                "controller {missing:#x}"
            );
        }
        row[8..12].copy_from_slice(&flags.to_be_bytes());
        row[0x40..0x44].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(projectile(&row, id, 0.001).is_err());
    }
}

#[test]
fn summon_projectiles_resolve_the_resident_bank_and_keep_original_motion() {
    use sha2::{Digest, Sha256};
    for (package, hash, bytes) in [
        (
            90,
            "36c13ecd431c6c314d030daf1bd1e87686a5d4fdb46cf3e971ddf50301f0ef8f",
            &[
                (10, 4),
                (11, 9),
                (13, 8),
                (17, 2),
                (19, 2),
                (20, 1),
                (21, 1),
                (22, 1),
                (64, 63),
                (65, 64),
                (68, 67),
                (69, 12),
                (72, 68),
                (73, 122),
                (92, 2),
                (94, 2),
                (100, 67),
                (101, 250),
                (139, 128),
            ][..],
        ),
        (
            92,
            "d072f6cf45ceb0cb49f0b39cb80dfa4b370bc4374fd2bb8dbf68f437ecda1cac",
            &[
                (8, 1),
                (9, 128),
                (11, 9),
                (13, 40),
                (17, 2),
                (19, 8),
                (20, 1),
                (22, 1),
                (40, 194),
                (41, 32),
                (44, 66),
                (45, 32),
                (64, 63),
                (65, 64),
                (68, 67),
                (69, 47),
                (72, 67),
                (73, 47),
                (93, 2),
                (100, 68),
                (101, 122),
                (104, 196),
                (105, 122),
                (136, 128),
                (137, 16),
                (138, 16),
                (139, 128),
                (145, 1),
                (146, 4),
            ][..],
        ),
    ] {
        let mut row = [0; 400];
        for &(offset, value) in bytes {
            row[offset] = value;
        }
        assert_eq!(format!("{:x}", Sha256::digest(row)), hash);
        let id = EffectId {
            bank: EffectBank::Magic(package),
            id: 1,
        };
        let recipe = projectile(&row, id, 0.001).unwrap();
        recipe.validate().unwrap();
        assert_eq!(recipe.birth_bank, Some(id.bank));
        assert!(recipe.ground_effect.is_none() && recipe.trail_effect.is_none());
        assert!(recipe.persist_after_hit && !recipe.clashable);
        if package == 90 {
            assert_eq!(
                (
                    recipe.lifetime,
                    recipe.shape.kind,
                    recipe.shape.radius,
                    recipe.shape.height
                ),
                (8, HitShapeKind::Cylinder, 140., 1000.)
            );
            assert_eq!(recipe.spawn_offset, [0., 500., 0.]);
            assert!(recipe.spawn_effect.is_none() && recipe.shadow.is_none());
            assert!(matches!(
                recipe.movement,
                ProjectileMovement::Ballistic {
                    velocity: [0., 0., 0.],
                    acceleration: [0., 0., 0.],
                    steering: None
                }
            ));
        } else {
            assert_eq!(
                (
                    recipe.lifetime,
                    recipe.shape.kind,
                    recipe.shape.radius,
                    recipe.shape.height
                ),
                (40, HitShapeKind::Box, 175., 175.)
            );
            assert_eq!(recipe.spawn_offset, [0., 1000., -1000.]);
            assert_eq!(
                recipe.spawn_effect,
                Some(EffectId {
                    bank: id.bank,
                    id: 2
                })
            );
            assert!(recipe.behavior.unlimited_range);
            assert!(matches!(
                recipe.movement,
                ProjectileMovement::Ballistic {
                    velocity: [0., -40., 40.],
                    acceleration: [0., 0., 0.],
                    steering: None
                }
            ));
            let shadow = recipe.shadow.unwrap();
            assert!(shadow.additive);
            assert_eq!(shadow.color, [128, 16, 16, 128]);
        }
        row[0xe] = 0;
        row[0xf] = 3;
        assert_eq!(
            projectile(&row, id, 0.001).unwrap().ground_effect,
            Some(EffectId {
                bank: EffectBank::Common,
                id: 3
            })
        );
    }
}

#[test]
fn thunder_tiger_native_allocator_rebinds_birth_bank_without_touching_ground() {
    // Original Magic117 projectile1; 205AC(mode2) calls60E60 after the400-byte copy.
    let mut row = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x49, 0x00, 0x14, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x3f, 0x40, 0x00, 0x00, 0x42, 0x20, 0x00, 0x00, 0x43, 0xc8, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let id = EffectId {
        bank: EffectBank::Magic(117),
        id: 1,
    };
    let decoded = projectile(&row, id, 0.1).unwrap();
    assert_eq!(
        decoded.spawn_effect,
        Some(EffectId {
            bank: id.bank,
            id: 2
        })
    );
    assert_eq!(decoded.lifetime, 20);
    assert_eq!(
        (
            decoded.shape.damage_kind,
            decoded.shape.hit_class,
            decoded.shape.reaction
        ),
        (0, 0, 1)
    );
    assert!(decoded.behavior.clamp_ground);
    // Only the two fields written by the native helper are rebound.
    row[0xe] = 0;
    row[0xf] = 17;
    let decoded = projectile(&row, id, 0.1).unwrap();
    assert_eq!(
        decoded.ground_effect,
        Some(EffectId {
            bank: EffectBank::Common,
            id: 17
        })
    );
    let unrelated = projectile(
        &row,
        EffectId {
            bank: EffectBank::Magic(104),
            id: 1,
        },
        0.1,
    )
    .unwrap();
    assert_eq!(
        unrelated.spawn_effect,
        Some(EffectId {
            bank: EffectBank::Common,
            id: 2
        })
    );
}

#[test]
fn unsubmitted_point_line_leaves_all_projectile_behavior_unchanged() {
    let mut row = [0u8; 400];
    row[8..12].copy_from_slice(&0x401u32.to_be_bytes());
    row[12..14].copy_from_slice(&40u16.to_be_bytes());
    row[0x2c..0x30].copy_from_slice(&16f32.to_be_bytes());
    row[0x44..0x48].copy_from_slice(&30f32.to_be_bytes());
    row[0x48..0x4c].copy_from_slice(&40f32.to_be_bytes());
    let id = EffectId {
        bank: EffectBank::Enemy(206),
        id: 0,
    };
    let original = serde_json::to_value(projectile(&row, id, 0.001).unwrap()).unwrap();
    row[8..12].copy_from_slice(&(0x401 | UNSUBMITTED_LINE).to_be_bytes());
    row[0x8c..0x90].copy_from_slice(&[128, 64, 32, 255]);
    for width in [0, 2, 5, 255] {
        row[0x9c] = width;
        assert_eq!(
            serde_json::to_value(projectile(&row, id, 0.001).unwrap()).unwrap(),
            original
        );
    }
    // Ground scattering is supported and independent of the unsubmitted line.
    row[8..12].copy_from_slice(&(0x8401 | UNSUBMITTED_LINE).to_be_bytes());
    let mut scattered = original;
    scattered["behavior"]["scatter_on_ground"] = true.into();
    assert_eq!(
        serde_json::to_value(projectile(&row, id, 0.001).unwrap()).unwrap(),
        scattered
    );
    // The line flag does not bypass rejection of an unsupported movement flag.
    row[8..12].copy_from_slice(&(0x501 | UNSUBMITTED_LINE).to_be_bytes());
    assert_eq!(
        projectile(&row, id, 0.001).unwrap_err().to_string(),
        "unsupported projectile movement flags 0x100"
    );
}

#[test]
#[ignore = "requires the original extracted disc"]
fn original_wind_projectiles_keep_motion_and_exclude_unsubmitted_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(root.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    for monster in [205u8, 206, 207] {
        let start = word(&usual, table + usize::from(monster) * 4).unwrap() as usize;
        let end = word(&usual, table + (usize::from(monster) + 1) * 4).unwrap() as usize;
        let enemy = compression::decode(&archive[start..end]).unwrap();
        let rows =
            super::enemy_inventory::offset_section(&enemy, word(&enemy, 0x1c8).unwrap() as usize)
                .unwrap();
        assert_eq!(rows.len(), if monster == 206 { 800 } else { 400 });
        for (id, row) in rows.chunks_exact(400).enumerate() {
            assert_eq!(
                word(row, 8).unwrap(),
                if monster == 206 { 0x6209 } else { 1 }
            );
            assert_eq!(&row[0x8c..0x90], &[0; 4]);
            assert_eq!(row[0x9c], 2);
            let recipe = projectile(
                row,
                EffectId {
                    bank: EffectBank::Enemy(monster),
                    id: id as u8,
                },
                0.001,
            )
            .unwrap();
            recipe.validate().unwrap();
            assert_eq!(
                recipe.lifetime,
                if monster == 206 && id == 0 { 120 } else { 60 }
            );
            if monster == 206 {
                let ProjectileMovement::Ballistic {
                    velocity,
                    acceleration,
                    steering: None,
                } = recipe.movement
                else {
                    panic!("Wind projectile motion");
                };
                assert_eq!(
                    velocity,
                    if id == 0 {
                        [0., -0.5, 36.]
                    } else {
                        [0., -20., 32.]
                    }
                );
                assert_eq!(
                    acceleration,
                    if id == 0 {
                        [0., -0.2, 0.]
                    } else {
                        [0., -0.1, 0.]
                    }
                );
                assert_eq!(recipe.spawn_offset, [0., 80., 0.]);
                assert!(
                    recipe.persist_after_hit && recipe.clashable && recipe.behavior.face_velocity
                );
                assert_eq!(
                    recipe.spawn_effect,
                    Some(EffectId {
                        bank: EffectBank::Enemy(206),
                        id: 2
                    })
                );
            }
        }
    }
    let rel = super::actions::Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
    for (at, target) in [
        (0x654, 0x125e4),
        (0x5c24, 0x76ee0),
        (0x684, 0x12c38),
        (0x688, 0x12aa0),
    ] {
        assert_eq!(rel.pointer(5, at).unwrap(), (1, target));
    }
    // The allocation is mode1/type23. Its only normal registration route
    // returns zero for that mode before the deferred draw table can reach it.
    for (at, instruction) in [
        (0x125f4, 0x480364b1),
        (0x13144, 0x38600004),
        (0x13148, 0x38800002),
        (0x1314c, 0x38a00017),
        (0x13150, 0x38c00004),
        (0x13168, 0x38a00001),
        (0x13170, 0x98bf0280),
        (0x48ad0, 0x88030012),
        (0x48ad4, 0x2c000017),
        (0x48adc, 0x88030280),
        (0x48ae0, 0x7c000775),
        (0x48ae4, 0x40820060),
        (0x48b44, 0x38600000),
        (0x48b48, 0x4e800020),
    ] {
        assert_eq!(
            word(rel.at((1, at)).unwrap(), 0).unwrap(),
            instruction,
            "{at:#x}"
        );
    }
}

#[cfg(test)]
mod thunder_blade_tests;

#[test]
fn lightning_allocators_replace_disabled_authored_enemy_banks() {
    let mut row = [0; 400];
    row[8..12].copy_from_slice(&0x409u32.to_be_bytes());
    row[12..14].copy_from_slice(&10u16.to_be_bytes());
    row[0x15] = 4;
    row[0x5c] = 2;
    row[0x5e] = 2;
    for package in [17, 18, 19] {
        let bank = EffectBank::Magic(package);
        let recipe = projectile(&row, EffectId { bank, id: 0 }, 1.).unwrap();
        assert_eq!(recipe.birth_bank, Some(bank));
        assert!(recipe.spawn_effect.is_none() && recipe.trail_effect.is_none());
    }
    assert!(
        projectile(
            &row,
            EffectId {
                bank: EffectBank::Magic(20),
                id: 0
            },
            1.
        )
        .is_err()
    );
}

#[test]
fn drake_breath_rows_bind_body_direction_and_keep_their_full_effect_lifecycle() {
    use sha2::{Digest, Sha256};
    for (id, hash, bytes) in [
        (
            0,
            "79891365b0e1e5a743dd5bcdb60cc1948d0481b370241ab5bd122f5b80c4e7a3",
            &[
                (0x9, 0x3),
                (0xa, 0x4),
                (0xb, 0x9),
                (0xd, 0x1e),
                (0xe, 0x2),
                (0xf, 0x7),
                (0x12, 0x1),
                (0x13, 0x3),
                (0x16, 0x1),
                (0x28, 0xc0),
                (0x29, 0xa0),
                (0x2c, 0x40),
                (0x2d, 0xa0),
                (0x3c, 0x41),
                (0x3d, 0xf0),
                (0x40, 0x3f),
                (0x41, 0x40),
                (0x44, 0x41),
                (0x45, 0xf0),
                (0x48, 0x41),
                (0x49, 0xf0),
                (0x5c, 0x2),
                (0x5d, 0x5),
                (0x5e, 0x2),
                (0x5f, 0x6),
                (0x7c, 0x41),
                (0x7d, 0xa0),
                (0x8b, 0x80),
                (0x91, 0x1),
                (0x92, 0x4),
                (0x9a, 0xd),
                (0x9b, 0xb),
                (0x9c, 0x2),
            ][..],
        ),
        (
            2,
            "1818f08f88cc82530c51e0b1609a566f19702eb3021115b35d3a08286713578a",
            &[
                (0x9, 0x3),
                (0xa, 0x4),
                (0xb, 0x19),
                (0xd, 0x3c),
                (0xe, 0x2),
                (0xf, 0xc),
                (0x12, 0x1),
                (0x13, 0x3),
                (0x15, 0x1),
                (0x16, 0x1),
                (0x3c, 0x41),
                (0x3d, 0xb8),
                (0x40, 0x3f),
                (0x41, 0x40),
                (0x44, 0x41),
                (0x45, 0xc8),
                (0x48, 0x41),
                (0x49, 0xc8),
                (0x5c, 0x2),
                (0x5d, 0xa),
                (0x5e, 0x2),
                (0x5f, 0xb),
                (0x64, 0xc1),
                (0x65, 0x20),
                (0x68, 0x41),
                (0x69, 0x20),
                (0x7c, 0x41),
                (0x7d, 0xa0),
                (0x8b, 0x80),
                (0x91, 0x1),
                (0x92, 0x4),
                (0x9a, 0xe),
                (0x9b, 0xb),
                (0x9c, 0x2),
            ][..],
        ),
    ] {
        let mut row = [0; 400];
        for &(offset, byte) in bytes {
            row[offset] = byte;
        }
        assert_eq!(format!("{:x}", Sha256::digest(row)), hash);
        let effect = EffectId {
            bank: EffectBank::Enemy(172),
            id,
        };
        let recipe = projectile(&row, effect, 0.001).unwrap();
        recipe.validate().unwrap();
        assert!(
            matches!(recipe.behavior.aim, ProjectileAim::Bones { toward, from: 11 } if toward == if id == 0 {13} else {14})
        );
        assert!(
            matches!(recipe.movement, ProjectileMovement::Directed { speed, .. } if speed == if id == 0 {30.} else {23.})
        );
        assert_eq!(recipe.lifetime, if id == 0 { 30 } else { 60 });
        assert_eq!(
            recipe.spawn_offset,
            if id == 0 { [0.; 3] } else { [0., -10., 10.] }
        );
        assert_eq!(
            recipe.shape.kind,
            if id == 0 {
                HitShapeKind::Box
            } else {
                HitShapeKind::Cylinder
            }
        );
        let first = if id == 0 { 5 } else { 10 };
        assert_eq!(
            [
                recipe.spawn_effect,
                recipe.trail_effect,
                recipe.ground_effect
            ],
            [first, first + 1, first + 2].map(|id| Some(EffectId {
                bank: effect.bank,
                id
            }))
        );
        assert!(recipe.persist_after_hit && !recipe.clashable && recipe.shadow.is_none());
        assert_eq!((recipe.active, recipe.trail_interval), (None, 1));
        // A target vector bypasses the bone query even if both flags were authored.
        let flags = word(&row, 8).unwrap() | 0x40000 | 0x200000;
        row[8..12].copy_from_slice(&flags.to_be_bytes());
        assert!(matches!(
            projectile(&row, effect, 0.001).unwrap().behavior.aim,
            ProjectileAim::Target
        ));
    }
}
