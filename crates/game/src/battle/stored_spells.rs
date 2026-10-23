//! Stored spells share scene preparation and independent release timing.
use super::{ActionResources, EffectResource, recoil};
use anyhow::{Context, Result};
use resonance_battle::{PreparedSpell, SpellOrigin};
use resonance_content::{battle_scene, prepared::Files};
use std::sync::Arc;

pub const ANGEL_FEATHERS: u16 = 56;
pub const ERUPTION: u16 = 67;
pub const STALAGMITE: u16 = 75;
pub const GRAVE: u16 = 77;
pub const SCENES: [(u16, u16); 36] = [
    (super::corrine::CORRINE, super::corrine::SCENE),
    (super::corrine::HEART, super::corrine::SCENE),
    (super::ground_pulse::SPREAD, 201),
    (super::aqua_laser::CATALOGUE, 203),
    (super::ground_pulse::AIR_THRUST, 209),
    (super::ground_pulse::CYCLONE, 210),
    (super::ground_pulse::AIR_BLADE, 211),
    (super::ground_pulse::GROUND_DASHER, 214),
    (super::ground_pulse::GRAVITY_WELL, 228),
    (super::ground_pulse::RAGING_MIST, 223),
    (super::prism::CATALOGUE, 232),
    (super::lance::CATALOGUE, 283),
    (super::lance::HOLY_LANCE, 253),
    (super::stored_recovery::HEAL, 238),
    (super::stored_recovery::CURE, 257),
    (super::stored_recovery::ACID_RAIN, 265),
    (102, 240),
    (103, 241),
    (105, 243),
    (106, 244),
    (108, 246),
    (109, 247),
    (111, 249),
    (112, 250),
    (117, 255),
    (118, 256),
    (120, 258),
    (121, 259),
    (122, 260),
    (123, 261),
    (super::photon::CATALOGUE, 251),
    (ANGEL_FEATHERS, 267),
    (ERUPTION, 205),
    (STALAGMITE, 213),
    (GRAVE, 215),
    (super::nurse::CATALOGUE, 237),
];
pub const TRANSITION_TICKS: u16 = 60;
// Initialization follows the transition; active age zero follows initialization.
pub(super) const ACTIVE_START_TICKS: u16 = 2;
// Contacts begin on the update after their scheduled creation.
pub(super) const CONTACT_INITIALIZATION_TICKS: u16 = 1;
pub(super) const SCENE_BOUNDARY_TICKS: u16 = 3;

/// Stored scenes share their transition, framing, and cleanup interval.
pub(super) fn scene_spell(
    scene: &battle_scene::Scene,
    origin: SpellOrigin,
    lifetime: u16,
) -> Result<PreparedSpell> {
    Ok(PreparedSpell {
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stage_tint(scene.stage_color, lifetime)?,
        camera: scene_camera(scene.camera, lifetime)?,
        ..PreparedSpell::new(
            origin,
            lifetime
                .checked_add(TRANSITION_TICKS + SCENE_BOUNDARY_TICKS)
                .context("stored scene duration overflow")?,
        )
    })
}

impl ActionResources {
    /// Bind authored contact artwork and cooldown to a scene's projectile.
    pub(super) fn scene_projectile(
        &mut self,
        files: &Files,
        source: &resonance_content::battle_projectile::Projectile,
        rule: &resonance_content::battle_action::ActionHit,
        recoil: &recoil::Parameters,
        bank: &EffectResource,
        banks: &[(u8, &EffectResource)],
    ) -> Result<Arc<resonance_battle::ProjectileDefinition>> {
        let mut hit = super::hit::authored(rule, recoil)?;
        if let Some(member) = rule.effect {
            hit.effect = Some(self.effect_member(bank, member)?);
        }
        let mut projectile = self.projectile(files, source, hit, banks)?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("scene projectile has no contact")?
            .cooldown = rule.cooldown;
        Ok(projectile)
    }
}

pub fn scene(catalogue: u16) -> Option<u16> {
    let catalogue = canonical(catalogue);
    SCENES
        .iter()
        .chain(&super::elemental_spells::SCENES)
        .chain(&super::advanced_ground_spells::SCENES)
        .chain(&super::colette_spells::SCENES)
        .find_map(|&(key, scene)| (key == catalogue).then_some(scene))
}

fn canonical(catalogue: u16) -> u16 {
    match catalogue {
        super::ground_pulse::KNIGHT_AIR_THRUST => super::ground_pulse::AIR_THRUST,
        214 => ERUPTION,
        218 => GRAVE,
        252 => super::colette_spells::JUDGMENT,
        _ => catalogue,
    }
}

pub fn prepare(
    files: &Files,
    catalogue: u16,
    bank: &EffectResource,
    common: &EffectResource,
    techniques: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let catalogue = canonical(catalogue);
    let scene_id = scene(catalogue).context("unknown ground spell")?;
    let scene: battle_scene::Scene = files.json(&battle_scene::path(scene_id))?;
    scene.validate()?;
    if let Some(recipe) = &scene.spell_recipe {
        return super::pattern_spells::prepare(files, &scene, recipe, bank, resources);
    }
    if super::colette_spells::SCENES
        .iter()
        .any(|&(key, _)| key == catalogue)
    {
        return super::colette_spells::prepare(files, catalogue, &scene, bank, resources);
    }
    if matches!(catalogue, super::corrine::CORRINE | super::corrine::HEART) {
        return super::corrine::prepare(files, &scene, bank, common, techniques, resources);
    }
    if super::elemental_spells::SCENES
        .iter()
        .any(|&(key, _)| key == catalogue)
    {
        return super::elemental_spells::prepare(files, catalogue, &scene, bank, resources);
    }
    if super::advanced_ground_spells::SCENES
        .iter()
        .any(|&(key, _)| key == catalogue)
    {
        return super::advanced_ground_spells::prepare(files, catalogue, &scene, bank, resources);
    }
    if catalogue == super::aqua_laser::CATALOGUE {
        return super::aqua_laser::prepare(files, &scene, bank, resources);
    }
    if catalogue == super::nurse::CATALOGUE {
        return super::nurse::prepare(&scene, bank, resources);
    }
    if matches!(
        catalogue,
        super::stored_recovery::HEAL
            | super::stored_recovery::CURE
            | super::stored_recovery::ACID_RAIN
            | 102
            | 103
            | 105
            | 106
            | 108
            | 109
            | 111
            | 112
            | 117
            | 118
            | 120..=123
    ) {
        return super::stored_recovery::prepare(catalogue, &scene, bank, techniques, resources);
    }
    if catalogue == super::photon::CATALOGUE {
        return super::photon::prepare(files, &scene, bank, resources);
    }
    if catalogue == super::prism::CATALOGUE {
        return super::prism::prepare(files, &scene, bank, resources);
    }
    if matches!(
        catalogue,
        super::lance::CATALOGUE | super::lance::HOLY_LANCE
    ) {
        return super::lance::prepare(files, &scene, bank, resources);
    }
    if matches!(
        catalogue,
        super::ground_pulse::SPREAD
            | super::ground_pulse::AIR_THRUST
            | super::ground_pulse::KNIGHT_AIR_THRUST
            | super::ground_pulse::CYCLONE
            | super::ground_pulse::AIR_BLADE
            | super::ground_pulse::GROUND_DASHER
            | super::ground_pulse::GRAVITY_WELL
            | super::ground_pulse::RAGING_MIST
    ) {
        return super::ground_pulse::prepare(files, catalogue, &scene, bank, resources);
    }
    let recoil = recoil::Parameters::load(files)?;
    if catalogue == ANGEL_FEATHERS {
        return angel_feathers(files, &scene, bank, resources, &recoil);
    }
    let (lifetime, schedule): (_, &[(u16, usize, usize)]) = match catalogue {
        ERUPTION => (210, &[(30, 2, 0), (60, 3, 1), (90, 2, 0)]),
        STALAGMITE => (245, &[(44, 0, 0), (60, 1, 1), (90, 2, 2)]),
        GRAVE => (
            215,
            &[(34, 0, 0), (78, 1, 1), (88, 1, 1), (98, 1, 1), (108, 1, 1)],
        ),
        _ => unreachable!(),
    };
    let mut projectiles = Vec::new();
    for &(at, row, rule_index) in schedule {
        let source = scene
            .projectiles
            .get(row)
            .context("missing ground spell pulse")?;
        let rule = source.hit_variant(rule_index)?;
        let projectile = resources.scene_projectile(
            files,
            &source.projectile,
            rule,
            &recoil,
            bank,
            &[(0, bank)],
        )?;
        projectiles.push(resonance_battle::SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + at + CONTACT_INITIALIZATION_TICKS,
            projectile,
        ));
    }
    let startup = resources.effect_member(bank, 1)?;
    Ok(PreparedSpell {
        projectiles,
        startup: Some((
            TRANSITION_TICKS + 1,
            if matches!(catalogue, ERUPTION | STALAGMITE) {
                resonance_battle::SpellStartup::DeferredEffect(startup)
            } else {
                resonance_battle::SpellStartup::Effect(startup)
            },
        )),
        ..scene_spell(
            &scene,
            match catalogue {
                ERUPTION | STALAGMITE => SpellOrigin::SceneGround { toward_caster: 1. },
                GRAVE => SpellOrigin::GroundAtRelease { toward_caster: 1. },
                _ => SpellOrigin::TargetGround,
            },
            lifetime,
        )?
    })
}

fn angel_feathers(
    files: &Files,
    scene: &battle_scene::Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
    recoil: &recoil::Parameters,
) -> Result<PreparedSpell> {
    const EMISSION_HEIGHT: f32 = 240.;
    let source = scene
        .projectiles
        .get(1)
        .context("missing Angel Feathers projectile")?;
    let rule = source.hit()?;
    let startup = resources.effect_member(bank, 1)?;
    let template = resources.scene_projectile(
        files,
        &source.projectile,
        rule,
        recoil,
        bank,
        &[(0, bank), (6, bank)],
    )?;
    let mut projectiles = Vec::with_capacity(3);
    for (at, velocity) in [
        (22, [-0.3, -0.2, 0.5]),
        (30, [0.1, 0.3, 0.5]),
        (38, [0.3, -0.2, 0.5]),
    ] {
        let mut projectile = (*template).clone();
        projectile.velocity = velocity;
        projectile.offset[1] += EMISSION_HEIGHT;
        projectile.validate()?;
        projectiles.push(resonance_battle::SpellProjectile::new(
            TRANSITION_TICKS + at + 1,
            Arc::new(projectile),
        ));
    }
    Ok(PreparedSpell {
        projectiles,
        startup: Some((
            TRANSITION_TICKS + 1,
            resonance_battle::SpellStartup::DeferredEffect(startup),
        )),
        ..scene_spell(scene, SpellOrigin::CasterRoot, scene.duration)?
    })
}

/// Active framing begins after the caster close-up and releases before scene cleanup.
pub(super) fn scene_camera(
    bounds: Option<battle_scene::CameraBounds>,
    lifetime: u16,
) -> Result<Option<resonance_battle::SpellCamera>> {
    const RESTORE_BEFORE_END: u16 = 45;
    let bounds = bounds.context("stored scene camera bounds are missing")?;
    bounds.validate()?;
    Ok(Some(resonance_battle::SpellCamera {
        bounds,
        duration: lifetime
            .checked_sub(RESTORE_BEFORE_END)
            .filter(|duration| *duration != 0)
            .context("stored scene is too short for its camera interval")?,
    }))
}

/// Scene color starts at initialization and begins returning before scene cleanup.
pub(super) fn stage_tint(
    color: Option<[u8; 3]>,
    lifetime: u16,
) -> Result<Option<resonance_battle::StageTint>> {
    const RESTORE_BEFORE_END: u16 = 30;
    Ok(Some(resonance_battle::StageTint {
        color: color.context("stored scene stage color is missing")?,
        duration: lifetime
            .checked_sub(RESTORE_BEFORE_END)
            .filter(|duration| *duration != 0)
            .context("stored scene is too short for its color channel")?,
    }))
}
