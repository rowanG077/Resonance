//! Colette's angel spells use the shared release clock and party benefits.
use super::{ActionResources, EffectResource, recoil, stored_spells};
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::{
    EffectAppearance, PreparedSpell, RecoveryTargets, SpellBenefit, SpellEffect,
    SpellEffectPlacement, SpellOrigin, SpellPolicy, SpellProjectile, SpellRecovery, SpellStartup,
    conditions::Buff,
};
use resonance_content::{battle_scene::Scene, prepared::Files};
use std::sync::Arc;

pub const HOLY_SONG: u16 = 57;
pub const JUDGMENT: u16 = 58;
pub const SACRIFICE: u16 = 59;
pub const HOLY_JUDGMENT: u16 = 251;
pub const SCENES: [(u16, u16); 4] = [
    (HOLY_SONG, 268),
    (JUDGMENT, 269),
    (SACRIFICE, 270),
    (HOLY_JUDGMENT, 296),
];

pub(super) fn prepare(
    files: &Files,
    catalogue: u16,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let active_ticks = match catalogue {
        HOLY_SONG => 160,
        JUDGMENT => 200,
        SACRIFICE => 330,
        HOLY_JUDGMENT => 380,
        _ => bail!("unknown angel spell {catalogue}"),
    };
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let mut artwork = bank.clone();
    artwork.members = match catalogue {
        SACRIFICE => vec![1, 3, 4, 5],
        HOLY_JUDGMENT => vec![1, 2, 3, 4],
        _ => vec![1, 2],
    };
    resources.effect(artwork)?;
    let mut spell = PreparedSpell {
        policy: Default::default(),
        summon: None,
        projectiles: vec![],
        effects: vec![],
        recovery: vec![],
        origin: SpellOrigin::CasterRoot,
        transition_ticks: stored_spells::TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, active_ticks)?,
        camera: stored_spells::scene_camera(scene.camera, active_ticks)?,
        duration: stored_spells::TRANSITION_TICKS
            + active_ticks
            + stored_spells::SCENE_BOUNDARY_TICKS,
        startup: Some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::Effect(appearance(1)),
        )),
        sound: None,
    };
    if matches!(catalogue, HOLY_SONG | HOLY_JUDGMENT) {
        let benefit_age = if catalogue == HOLY_JUDGMENT { 210 } else { 90 };
        spell.recovery.push(SpellRecovery {
            at: at(benefit_age),
            benefit: SpellBenefit::Buff(Buff::HolySong),
            targets: RecoveryTargets::LivingAllies,
            effect: Some((appearance(2), Default::default())),
        });
    }
    if matches!(catalogue, JUDGMENT | HOLY_JUDGMENT) {
        const STRIKE_COUNT: usize = 17;
        const STRIKE_INTERVAL: u16 = 6;
        const PATTERN_HEADING: f32 = 20.;
        let (first_strike, strike_effect) = if catalogue == HOLY_JUDGMENT {
            (210, 3)
        } else {
            (30, 2)
        };
        ensure!(
            scene.strike_points.len() == STRIKE_COUNT,
            "Judgment scene has an incomplete strike pattern"
        );
        let projectile = contact(files, scene, bank, resources, 1)?;
        spell.origin = SpellOrigin::SceneFixed { position: [0.; 3] };
        if catalogue == HOLY_JUDGMENT {
            const PORTRAIT_HOLD: u16 = 71;
            // Two invisible endpoints surround the 102 visible presentation updates.
            const PORTRAIT_DURATION: u16 = 104;
            spell.policy = SpellPolicy::Cutin {
                at: at(90),
                catalogue,
                duration: PORTRAIT_DURATION,
                hold_ticks: PORTRAIT_HOLD,
                reveal: Some((at(150), "Holy Judgment")),
            };
            spell.startup = None;
            for (age, member) in [(130, 4), (150, 1)] {
                spell.effects.push(SpellEffect {
                    at: at(age),
                    appearance: appearance(member),
                    placement: SpellEffectPlacement::CasterGround,
                    captured_heading_offset: None,
                });
            }
        }
        let (sin, cos) = PATTERN_HEADING.to_radians().sin_cos();
        for (index, &[x, y, z]) in scene.strike_points.iter().enumerate() {
            let at = at(first_strike + STRIKE_INTERVAL * index as u16);
            let offset = [cos * x + sin * z, y, -sin * x + cos * z];
            let mut contact = SpellProjectile::new(at, Arc::clone(&projectile));
            contact.origin_offset = offset;
            spell.projectiles.push(contact);
            spell.effects.push(SpellEffect {
                at,
                appearance: appearance(strike_effect),
                placement: SpellEffectPlacement::Scene(offset),
                captured_heading_offset: Some(PATTERN_HEADING),
            });
        }
    } else if catalogue == SACRIFICE {
        const BURST_AGE: u16 = 135;
        const BENEFIT_AGE: u16 = 150;
        const LAST_CONTACT_AGE: u16 = 195;
        spell.origin = SpellOrigin::SceneFixed { position: [0.; 3] };
        spell.startup = Some((
            stored_spells::TRANSITION_TICKS + 1,
            SpellStartup::CasterGroundEffect(appearance(1)),
        ));
        for (tick, member, placement) in [
            (
                stored_spells::TRANSITION_TICKS + 1,
                3,
                SpellEffectPlacement::CasterCenter,
            ),
            (at(BURST_AGE), 4, SpellEffectPlacement::CasterCenter),
            (at(BURST_AGE), 5, SpellEffectPlacement::EnemyCenters),
        ] {
            spell.effects.push(SpellEffect {
                at: tick,
                appearance: appearance(member),
                placement,
                captured_heading_offset: None,
            });
        }
        for (age, member) in [(BENEFIT_AGE, 1), (LAST_CONTACT_AGE, 2)] {
            spell.projectiles.push(SpellProjectile::new(
                at(age),
                contact(files, scene, bank, resources, member)?,
            ));
        }
        spell.recovery.push(SpellRecovery {
            at: at(BENEFIT_AGE),
            benefit: SpellBenefit::Sacrifice,
            targets: RecoveryTargets::Caster,
            effect: None,
        });
    }
    Ok(spell)
}

fn at(age: u16) -> u16 {
    stored_spells::TRANSITION_TICKS + stored_spells::ACTIVE_START_TICKS + age
}

fn contact(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
    member: usize,
) -> Result<Arc<resonance_battle::ProjectileDefinition>> {
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let source = scene
        .projectiles
        .get(member)
        .context("angel scene has no requested contact")?;
    let rule = source.hit()?;
    let recoil = recoil::Parameters::load(files)?;
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
        .context("angel projectile has no contact")?
        .cooldown = rule.cooldown;
    Ok(projectile)
}
