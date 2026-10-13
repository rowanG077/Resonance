//! Ordinary summons prepare their scene, contacts and party blessing together.
use super::{ActionResources, EffectResource, recoil, stored_spells, voice};
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::{
    EffectAppearance, PreparedSpell, PreparedSummon, Sound, SpellOrigin, SummonBlessing,
    SummonContact, SummonEffect, SummonEntryDefinition, SummonHeading, SummonOrigin, SummonPoint,
};
use resonance_content::{battle_scene, prepared::Files};
use std::sync::Arc;

const EFREET: u16 = 235;
const UNDINE: u16 = 236;
const SYLPH: u16 = 237;
const LUNA: u16 = 238;
const GNOME: u16 = 239;
const CELSIUS: u16 = 240;
const VOLT: u16 = 241;
const SHADOW: u16 = 242;
const ORIGIN: u16 = 243;
const MAXWELL: u16 = 244;
const ACTIVE_START: u16 = 1;
const ENTRY_EFFECT: u16 = 36;
const ENTRY_SOUND: u16 = 122;
const HEALING_EFFECT: u16 = 20;
const BLESSING_RADIUS: f32 = 512.;
const CELSIUS_ICE_SOUND: u16 = 126;
const CELSIUS_ICE_SOUND_TICKS: [u16; 6] = [126, 134, 142, 150, 158, 166];
const PATTERN_HEADING: f32 = 20.;
const PATTERN_FIRST_STRIKE: u16 = 140;

pub const SCENES: [(u16, u16); 10] = [
    (EFREET, 284),
    (UNDINE, 285),
    (SYLPH, 286),
    (LUNA, 290),
    (GNOME, 287),
    (CELSIUS, 288),
    (VOLT, 289),
    (SHADOW, 291),
    (ORIGIN, 293),
    (MAXWELL, 292),
];

pub fn scene(catalogue: u16) -> Option<u16> {
    SCENES
        .iter()
        .find_map(|&(key, scene)| (key == catalogue).then_some(scene))
}

pub fn prepare(
    files: &Files,
    catalogue: u16,
    bank: &EffectResource,
    common: &EffectResource,
    techniques: &EffectResource,
    resources: &mut ActionResources,
    sound: &mut impl FnMut(voice::Sound) -> Result<Option<Sound>>,
) -> Result<PreparedSpell> {
    let scene_id = scene(catalogue).context("unknown summon")?;
    let scene: battle_scene::Scene = files.json(&battle_scene::path(scene_id))?;
    let healing = EffectAppearance {
        resource: techniques.resource,
        member: HEALING_EFFECT,
    };
    let recipe = recipe(catalogue, healing)?;
    let recoil = recoil::Parameters::load(files)?;
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let arrival = SummonPoint::Arrival([0.; 3]);
    let captured = SummonHeading::Captured(0.);
    let mut effects = vec![
        SummonEffect {
            at: 0,
            appearance: appearance(2),
            point: SummonPoint::World,
            heading: captured,
        },
        SummonEffect {
            at: 0,
            appearance: appearance(1),
            point: arrival,
            heading: captured,
        },
    ];
    if matches!(catalogue, VOLT | SHADOW | ORIGIN) {
        effects.swap(0, 1);
    }
    const WAVES: [[f32; 3]; 4] = [
        [0., 0., 400.],
        [-300., 0., -250.],
        [300., 0., -250.],
        [0.; 3],
    ];
    if catalogue == UNDINE {
        effects.remove(0);
        effects[0].point = SummonPoint::World;
        for (index, offset) in WAVES.into_iter().enumerate() {
            effects.push(SummonEffect {
                at: ACTIVE_START + 90 + 45 * index as u16,
                appearance: appearance(2),
                point: SummonPoint::Arrival(offset),
                heading: SummonHeading::Caster,
            });
        }
        let mut healing_effect = techniques.clone();
        healing_effect.members = vec![healing.member];
        resources.effect(healing_effect)?;
    }
    const ICE_LANES: [f32; 4] = [45., 135., 225., 315.];
    let announcements: &[(u16, u16)] = match catalogue {
        EFREET => &[(66, 1347)],
        UNDINE => &[(105, 1291)],
        SYLPH => &[(54, 1308), (113, 1323), (187, 1331)],
        LUNA => &[(ACTIVE_START + 90, 1412)],
        GNOME => &[(37, 1364)],
        CELSIUS => &[(75, 1380)],
        SHADOW => &[(80, 1400)],
        ORIGIN => &[(85, 1437)],
        MAXWELL => &[(ACTIVE_START + 90, 1428)],
        _ => &[],
    };
    let mut voices = Vec::new();
    for &(at, stream) in announcements {
        if let Some(announcement) = sound(voice::Sound::Stream(stream))? {
            voices.push((at, announcement));
        }
    }
    let mut sounds = Vec::new();
    if catalogue == CELSIUS {
        if let Some(ice) = sound(voice::Sound::Cue(CELSIUS_ICE_SOUND))? {
            sounds.extend(CELSIUS_ICE_SOUND_TICKS.map(|at| (at, ice)));
        }
        effects.remove(0);
        for heading in ICE_LANES {
            effects.push(SummonEffect {
                at: ACTIVE_START + 150,
                appearance: appearance(2),
                point: arrival,
                heading: SummonHeading::Captured(heading),
            });
        }
    }
    let mut projectile = |row: usize| -> Result<Arc<resonance_battle::ProjectileDefinition>> {
        let source = scene
            .projectiles
            .get(row)
            .context("missing summon contact")?;
        let rule = source.hit()?;
        let mut hit = super::hit::authored(rule, &recoil)?;
        if let Some(member) = rule.effect {
            let mut effect = bank.clone();
            effect.members = vec![member];
            resources.effect(effect)?;
            hit.effect = Some(appearance(member));
        }
        let mut projectile = resources.projectile(
            files,
            &source.projectile,
            hit,
            &[(0, common), (1, techniques), (2, bank), (6, bank)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("summon projectile has no contact")?
            .cooldown = rule.cooldown;
        Ok(projectile)
    };
    let mut contacts = Vec::new();
    for (index, &(tick, row)) in recipe.pulses.iter().enumerate() {
        let projectile = projectile(row)?;
        let headings: &[f32] = if catalogue == CELSIUS {
            &ICE_LANES
        } else {
            &[0.]
        };
        for &heading in headings {
            contacts.push(SummonContact {
                // Active callback zero follows the initialization update.
                at: ACTIVE_START + tick,
                projectile: Arc::clone(&projectile),
                point: if catalogue == UNDINE {
                    SummonPoint::Arrival(WAVES[index])
                } else {
                    arrival
                },
                heading: if matches!(catalogue, SYLPH | CELSIUS) {
                    SummonHeading::Captured(heading)
                } else {
                    SummonHeading::Caster
                },
            });
        }
    }
    if matches!(catalogue, LUNA | MAXWELL) {
        ensure!(
            !scene.strike_points.is_empty(),
            "missing summon strike pattern"
        );
        let projectile = projectile(1)?;
        let (interval, member) = if catalogue == LUNA { (6, 2) } else { (15, 3) };
        let heading = SummonHeading::Captured(PATTERN_HEADING);
        effects = vec![SummonEffect {
            at: 0,
            appearance: appearance(1),
            point: SummonPoint::World,
            heading,
        }];
        let (sin, cos) = PATTERN_HEADING.to_radians().sin_cos();
        for (index, &[x, y, z]) in scene.strike_points.iter().enumerate() {
            let at = ACTIVE_START + PATTERN_FIRST_STRIKE + interval * index as u16;
            let point = SummonPoint::Arrival([cos * x + sin * z, y, -sin * x + cos * z]);
            effects.push(SummonEffect {
                at,
                appearance: appearance(member),
                point,
                heading,
            });
            contacts.push(SummonContact {
                at,
                projectile: Arc::clone(&projectile),
                point,
                heading: if catalogue == LUNA {
                    SummonHeading::Caster
                } else {
                    heading
                },
            });
        }
    }
    let mut scene_effects = bank.clone();
    scene_effects.members = effects
        .iter()
        .map(|effect| effect.appearance.member)
        .collect();
    resources.effect(scene_effects)?;
    let mut entry_effects = common.clone();
    entry_effects.members = vec![ENTRY_EFFECT];
    resources.effect(entry_effects)?;
    let summon = PreparedSummon {
        entry: SummonEntryDefinition {
            effect: EffectAppearance {
                resource: common.resource,
                member: ENTRY_EFFECT,
            },
            sound: sound(voice::Sound::Cue(ENTRY_SOUND))?,
        },
        origin: recipe.origin,
        effects,
        contacts,
        voices,
        sounds,
        scale: 1.,
        title: recipe.title.into(),
        focus_distance: recipe.focus_distance,
        focus_ticks: recipe.focus_ticks,
        focus_world: matches!(catalogue, UNDINE | CELSIUS | LUNA | MAXWELL),
        blessing: recipe.blessing,
        blessing_radius: BLESSING_RADIUS,
    };
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: Some(Arc::new(summon)),
        effects: vec![],
        projectiles: vec![],
        recovery: vec![],
        origin: SpellOrigin::CasterRoot,
        transition_ticks: 0,
        stage_tint: stored_spells::stage_tint(
            Some(scene.stage_color.context("missing summon stage color")?),
            recipe.lifetime,
        )?,
        camera: stored_spells::scene_camera(
            Some(scene.camera.context("missing summon camera bounds")?),
            recipe.lifetime,
        )?,
        // Active(0) and cleanup follow the initialization update.
        duration: recipe.lifetime + 2,
        startup: None,
        sound: None,
    })
}

struct Recipe {
    title: &'static str,
    lifetime: u16,
    origin: SummonOrigin,
    pulses: &'static [(u16, usize)],
    focus_distance: f32,
    focus_ticks: u16,
    blessing: SummonBlessing,
}

fn recipe(catalogue: u16, healing: EffectAppearance) -> Result<Recipe> {
    let direction = |distance| SummonOrigin::TargetDirection {
        height: 0.,
        distance,
    };
    let ground = |height| SummonOrigin::TargetGround {
        height,
        nudge: 1.,
        direction_threshold: 0.5,
    };
    Ok(match catalogue {
        EFREET => Recipe {
            title: "-Efreet-",
            lifetime: 315,
            origin: direction(-150.),
            pulses: &[(128, 2)],
            focus_distance: 2150.,
            focus_ticks: 120,
            blessing: SummonBlessing::Attack(15),
        },
        UNDINE => Recipe {
            title: "-Undine-",
            lifetime: 385,
            origin: ground(0.),
            pulses: &[(115, 1), (160, 1), (205, 1), (250, 1)],
            focus_distance: 1850.,
            focus_ticks: 120,
            blessing: SummonBlessing::Heal {
                percent: 50,
                effect: healing,
            },
        },
        SYLPH => Recipe {
            title: "-Sylph-",
            lifetime: 375,
            origin: direction(-150.),
            pulses: &[
                (103, 1),
                (166, 2),
                (170, 2),
                (174, 2),
                (178, 2),
                (182, 2),
                (235, 3),
            ],
            focus_distance: 2000.,
            focus_ticks: 150,
            blessing: SummonBlessing::Speed,
        },
        LUNA => Recipe {
            title: "-Luna-",
            lifetime: 335,
            origin: SummonOrigin::World,
            pulses: &[],
            focus_distance: 2150.,
            focus_ticks: 130,
            blessing: SummonBlessing::Magic(15),
        },
        GNOME => Recipe {
            title: "-Gnome-",
            lifetime: 255,
            origin: ground(0.),
            pulses: &[(110, 1), (115, 2)],
            focus_distance: 2450.,
            focus_ticks: 120,
            blessing: SummonBlessing::Defense(15),
        },
        CELSIUS => Recipe {
            title: "-Celsius-",
            lifetime: 310,
            origin: SummonOrigin::World,
            pulses: &[(156, 1), (176, 2)],
            focus_distance: 2000.,
            focus_ticks: 150,
            blessing: SummonBlessing::Accuracy(15),
        },
        VOLT => Recipe {
            title: "-Volt-",
            lifetime: 315,
            origin: ground(300.),
            pulses: &[(165, 1)],
            focus_distance: 2650.,
            focus_ticks: 120,
            blessing: SummonBlessing::PhysicalProtection,
        },
        SHADOW => Recipe {
            title: "-Shadow-",
            lifetime: 330,
            origin: direction(-200.),
            pulses: &[(120, 1)],
            focus_distance: 1850.,
            focus_ticks: 120,
            blessing: SummonBlessing::MagicalProtection,
        },
        ORIGIN => Recipe {
            title: "-Origin-",
            lifetime: 315,
            origin: direction(-400.),
            pulses: &[(120, 1)],
            focus_distance: 2250.,
            focus_ticks: 120,
            blessing: SummonBlessing::AttackDefense(10),
        },
        MAXWELL => Recipe {
            title: "-Maxwell-",
            lifetime: 445,
            origin: SummonOrigin::World,
            pulses: &[],
            focus_distance: 2150.,
            focus_ticks: 135,
            blessing: SummonBlessing::Magic(20),
        },
        _ => bail!("unknown summon {catalogue}"),
    })
}
