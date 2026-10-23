//! Seven sword impacts retain the ground point and orientation chosen at scene entry.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, PreparedSpell, SpellEffect, SpellOrigin, SpellProjectile,
};
use resonance_content::{battle_scene::Scene, prepared::Files};

pub const CATALOGUE: u16 = 94;
const LIFETIME: u16 = 210;
const HEADING_OFFSET: f32 = 20.;
const IMPACTS: [(u16, [f32; 2]); 7] = [
    (30, [-207., -445.]),
    (38, [408., -204.]),
    (46, [69., 328.]),
    (54, [-300., 62.]),
    (62, [37., -234.]),
    (70, [188., 43.]),
    (94, [-30., -26.]),
];

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let recoil = recoil::Parameters::load(files)?;
    let mut bank = bank.clone();
    bank.members = (1..=10).collect();
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let mut effects = vec![SpellEffect {
        at: TRANSITION_TICKS + 1,
        appearance: appearance(1),
        placement: resonance_battle::SpellEffectPlacement::Scene([0.; 3]),
        deferred: true,
        captured_heading_offset: Some(HEADING_OFFSET),
    }];
    let mut projectiles = Vec::with_capacity(IMPACTS.len());
    let (sin, cos) = HEADING_OFFSET.to_radians().sin_cos();
    for (index, (tick, [x, z])) in IMPACTS.into_iter().enumerate() {
        let final_impact = index + 1 == IMPACTS.len();
        let source = scene
            .projectiles
            .get(if final_impact { 2 } else { 1 })
            .context("missing Prism Sword contact")?;
        let rule = source.hit()?;
        let projectile = resources.scene_projectile(
            files,
            &source.projectile,
            rule,
            &recoil,
            &bank,
            &[(0, &bank), (2, &bank)],
        )?;
        let at = TRANSITION_TICKS + ACTIVE_START_TICKS + tick;
        let offset = [cos * x + sin * z, 0., cos * z - sin * x];
        for member in [if final_impact { 3 } else { 2 }, index as u16 + 4] {
            effects.push(SpellEffect {
                at,
                appearance: appearance(member),
                placement: resonance_battle::SpellEffectPlacement::Scene(offset),
                deferred: true,
                captured_heading_offset: Some(HEADING_OFFSET),
            });
        }
        projectiles.push(SpellProjectile {
            at: at + 1,
            definition: projectile,
            origin_offset: offset,
            launch: Default::default(),
        });
    }
    resources.effect(bank)?;
    Ok(PreparedSpell {
        effects,
        projectiles,
        ..stored_spells::scene_spell(
            scene,
            SpellOrigin::SceneGround { toward_caster: 1. },
            LIFETIME,
        )?
    })
}
