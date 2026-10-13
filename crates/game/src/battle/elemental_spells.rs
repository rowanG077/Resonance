//! Stored elemental spells share resource preparation and explicit contact schedules.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result, bail};
use resonance_battle::{
    EffectAppearance, PreparedSpell, SpellEffect, SpellLaunch, SpellOrigin, SpellPolicy,
    SpellProjectile, SpellStartup,
};
use resonance_content::{battle_scene::Scene, prepared::Files};
use std::sync::Arc;

pub const EXPLOSION: u16 = 68;
pub const FLAME_LANCE: u16 = 69;
pub const SPARK_WAVE: u16 = 79;
pub const INDIGNATION: u16 = 80;
pub const THUNDER_BLADE: u16 = 81;
pub const ICE_TORNADO: u16 = 83;
pub const FREEZE_LANCER: u16 = 84;
pub const SPIRAL_FLARE: u16 = 88;
pub const THUNDER_ARROW: u16 = 89;
pub const ABSOLUTE: u16 = 92;
pub const METEOR_STORM: u16 = 95;
pub const RAY: u16 = 114;
pub const KNIGHT_THUNDER_BLADE: u16 = 220;
pub const SACRED_LIGHT: u16 = 222;
pub const DARK_SPHERE: u16 = 246;
pub const INDIGNATION_JUDGMENT: u16 = 248;

pub const SCENES: [(u16, u16); 16] = [
    (EXPLOSION, 206),
    (FLAME_LANCE, 207),
    (SPARK_WAVE, 217),
    (INDIGNATION, 218),
    (THUNDER_BLADE, 219),
    (ICE_TORNADO, 221),
    (FREEZE_LANCER, 222),
    (SPIRAL_FLARE, 226),
    (THUNDER_ARROW, 227),
    (ABSOLUTE, 230),
    (METEOR_STORM, 233),
    (RAY, 252),
    (KNIGHT_THUNDER_BLADE, 219),
    (SACRED_LIGHT, 263),
    (DARK_SPHERE, 278),
    (INDIGNATION_JUDGMENT, 280),
];

pub(super) fn prepare(
    files: &Files,
    catalogue: u16,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    if catalogue == DARK_SPHERE {
        return super::photon::prepare(files, scene, bank, resources);
    }
    // Each tuple selects a birth age, a projectile, and its damage rule.
    let (lifetime, contacts): (_, &[(u16, usize, usize)]) = match catalogue {
        EXPLOSION => (190, &[(3, 1, 0), (65, 2, 1)]),
        FLAME_LANCE => (190, &[(15, 1, 0), (90, 2, 1)]),
        SPARK_WAVE => (180, &[(30, 1, 0)]),
        INDIGNATION => (290, &[(170, 0, 0)]),
        INDIGNATION_JUDGMENT => (
            315,
            &[
                (15, 1, 0),
                (80, 2, 1),
                (150, 3, 2),
                (160, 1, 0),
                (220, 4, 3),
            ],
        ),
        THUNDER_BLADE | KNIGHT_THUNDER_BLADE => (170, &[(10, 0, 0), (80, 2, 1)]),
        ICE_TORNADO => (200, &[(45, 1, 0)]),
        FREEZE_LANCER => (
            210,
            &[
                (20, 1, 0),
                (28, 1, 0),
                (36, 1, 0),
                (44, 1, 0),
                (52, 1, 0),
                (60, 1, 0),
            ],
        ),
        SPIRAL_FLARE => (160, &[(30, 1, 0)]),
        THUNDER_ARROW => (195, &[(60, 1, 0)]),
        ABSOLUTE => (210, &[(62, 1, 0), (120, 2, 1)]),
        SACRED_LIGHT => (210, &[]),
        METEOR_STORM => (
            330,
            &[
                (15, 1, 0),
                (30, 1, 0),
                (45, 1, 0),
                (60, 1, 0),
                (75, 1, 0),
                (90, 1, 0),
                (105, 1, 0),
                (120, 1, 0),
                (135, 1, 0),
                (150, 1, 0),
                (165, 1, 0),
                (180, 1, 0),
                (195, 1, 0),
                (210, 1, 0),
            ],
        ),
        RAY => (
            230,
            &[
                (60, 1, 0),
                (68, 1, 0),
                (76, 1, 0),
                (84, 1, 0),
                (92, 1, 0),
                (100, 1, 0),
                (108, 1, 0),
                (116, 1, 0),
                (124, 1, 0),
            ],
        ),
        _ => bail!("unknown elemental spell {catalogue}"),
    };
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let mut required = bank.clone();
    required.members = match catalogue {
        THUNDER_ARROW | RAY => vec![1, 2],
        METEOR_STORM => vec![1, 3],
        ABSOLUTE => vec![1, 2, 3],
        _ => vec![1],
    };
    resources.effect(required)?;
    let recoil = recoil::Parameters::load(files)?;
    let mut projectiles = Vec::with_capacity(contacts.len());
    for &(tick, row, rule_index) in contacts {
        let source = scene
            .projectiles
            .get(row)
            .with_context(|| format!("spell {catalogue} has no projectile {row}"))?;
        let rule = source.hit_variant(rule_index)?;
        let mut hit = super::hit::authored(rule, &recoil)?;
        if let Some(member) = rule.effect {
            let mut required = bank.clone();
            required.members = vec![member];
            resources.effect(required)?;
            hit.effect = Some(appearance(member));
        }
        let mut projectile = resources.projectile(
            files,
            &source.projectile,
            hit,
            &[(0, bank), (2, bank), (6, bank)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("elemental projectile has no contact")?
            .cooldown = rule.cooldown;
        projectiles.push(SpellProjectile::new(at(tick), projectile));
    }
    let mut spell = PreparedSpell {
        summon: None,
        policy: if catalogue == SPARK_WAVE {
            SpellPolicy::TrackGround {
                start: at(31),
                end: at(110),
                speed: 1.25,
            }
        } else {
            SpellPolicy::Static
        },
        projectiles,
        effects: vec![],
        recovery: vec![],
        origin: match catalogue {
            METEOR_STORM | SACRED_LIGHT => SpellOrigin::SceneFixed { position: [0.; 3] },
            SPARK_WAVE => SpellOrigin::SceneTargetPlane {
                height: 200.,
                toward_caster: 1.,
                body_center: false,
            },
            SPIRAL_FLARE => SpellOrigin::SceneCasterAhead {
                distance: 80.,
                height: 100.,
            },
            FREEZE_LANCER => SpellOrigin::SceneCasterRing {
                distance: 100.,
                minimum_height: 170.,
            },
            _ => SpellOrigin::SceneGround { toward_caster: 1. },
        },
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(
            Some(
                scene
                    .stage_color
                    .context("elemental scene has no stage color")?,
            ),
            lifetime,
        )?,
        camera: stored_spells::scene_camera(
            Some(
                scene
                    .camera
                    .context("elemental scene has no camera bounds")?,
            ),
            lifetime,
        )?,
        duration: TRANSITION_TICKS + lifetime + SCENE_BOUNDARY_TICKS,
        startup: Some((TRANSITION_TICKS + 1, SpellStartup::Effect(appearance(1)))),
        sound: None,
    };
    if catalogue == INDIGNATION_JUDGMENT {
        const PORTRAIT_DURATION: u16 = 64;
        const PORTRAIT_HOLD: u16 = 61;
        spell.policy = SpellPolicy::Cutin {
            at: at(185),
            catalogue,
            duration: PORTRAIT_DURATION,
            hold_ticks: PORTRAIT_HOLD,
            reveal: None,
        };
    }
    if catalogue == FREEZE_LANCER {
        const RING_RADIUS: f32 = 150.;
        for (projectile, sector) in spell.projectiles.iter_mut().zip([0, 2, 4, 1, 3, 5]) {
            let (sin, cos) = (sector as f32 * std::f32::consts::PI / 3.).sin_cos();
            projectile.origin_offset = [RING_RADIUS * sin, RING_RADIUS * cos, 0.];
            projectile.launch = SpellLaunch::ForwardCone {
                minimum_dot: 0.5,
                vertical_limit: 0.35,
            };
        }
    }
    if catalogue == ABSOLUTE {
        const CAPTURE_RADIUS: f32 = 300.;
        spell.policy = SpellPolicy::CaptureRecipient {
            at: at(62),
            radius: CAPTURE_RADIUS,
        };
        for (tick, member) in [(62, 2), (120, 3)] {
            spell.effects.push(SpellEffect {
                at: at(tick),
                appearance: appearance(member),
                placement: resonance_battle::SpellEffectPlacement::Scene([0.; 3]),
                captured_heading_offset: None,
            });
        }
    }
    if catalogue == THUNDER_ARROW {
        const HEADING_OFFSET: f32 = 22.5;
        const SATELLITE_RADIUS: f32 = 300.;
        // The central effect shares the captured caster heading; satellites face forward.
        spell.startup = None;
        spell.effects.push(SpellEffect {
            at: TRANSITION_TICKS + 1,
            appearance: appearance(1),
            placement: resonance_battle::SpellEffectPlacement::Scene([0.; 3]),
            captured_heading_offset: Some(HEADING_OFFSET),
        });
        for satellite in 0..3 {
            let angle = HEADING_OFFSET + satellite as f32 * 120.;
            let (sin, cos) = angle.to_radians().sin_cos();
            spell.effects.push(SpellEffect {
                at: at(10),
                appearance: appearance(2),
                placement: resonance_battle::SpellEffectPlacement::Scene([
                    SATELLITE_RADIUS * sin,
                    0.,
                    SATELLITE_RADIUS * cos,
                ]),
                captured_heading_offset: None,
            });
        }
    }
    if matches!(catalogue, RAY | METEOR_STORM) {
        let (heading_offset, effect_member): (f32, _) =
            if catalogue == RAY { (20., 2) } else { (45., 3) };
        if scene.strike_points.len() != spell.projectiles.len() {
            bail!(
                "spell {catalogue} requires {} authored strike points",
                spell.projectiles.len()
            );
        }
        spell.startup = None;
        spell.effects.push(SpellEffect {
            at: TRANSITION_TICKS + 1,
            appearance: appearance(1),
            placement: resonance_battle::SpellEffectPlacement::Scene([0.; 3]),
            captured_heading_offset: Some(heading_offset),
        });
        let (sin, cos) = heading_offset.to_radians().sin_cos();
        for (projectile, &[x, y, z]) in spell.projectiles.iter_mut().zip(&scene.strike_points) {
            let offset = [x * cos + z * sin, y, z * cos - x * sin];
            projectile.origin_offset = offset;
            projectile.launch = SpellLaunch::Captured(heading_offset);
            spell.effects.push(SpellEffect {
                at: projectile.at,
                appearance: appearance(effect_member),
                placement: resonance_battle::SpellEffectPlacement::Scene(offset),
                captured_heading_offset: Some(heading_offset),
            });
        }
    }
    Ok(spell)
}

fn at(tick: u16) -> u16 {
    TRANSITION_TICKS + ACTIVE_START_TICKS + tick
}
