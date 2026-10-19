//! Nurse sends a model ring across the arena, then restores the living party.
use super::{
    ActionResources, EffectResource,
    stored_spells::{SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::Result;
use resonance_battle::{
    EffectAppearance, PreparedSpell, RecoveryTargets, SpellEffect, SpellEffectPlacement,
    SpellOrigin, SpellRecovery, SpellStartup,
};

pub const CATALOGUE: u16 = 99;
const MODEL_START_SCENE_TICK: u16 = 1;
const RECOVERY_SCENE_TICK: u16 = 122;
const ACTIVE_SCENE_TICKS: u16 = 250;
const CLEANUP_SCENE_TICK: u16 = ACTIVE_SCENE_TICKS + SCENE_BOUNDARY_TICKS;

pub fn prepare(
    scene: &resonance_content::battle_scene::Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let effect = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let mut artwork = bank.clone();
    artwork.members = (1..=6).collect();
    resources.effect(artwork)?;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![SpellEffect {
            at: TRANSITION_TICKS + 1,
            appearance: effect(1),
            placement: SpellEffectPlacement::CasterCenter,
            deferred: false,
            captured_heading_offset: None,
        }],
        projectiles: Vec::new(),
        recovery: vec![SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: TRANSITION_TICKS + RECOVERY_SCENE_TICK,
            benefit: resonance_battle::SpellBenefit::Hp(40),
            targets: RecoveryTargets::LivingAllies,
            effect: Some((effect(6), Default::default())),
        }],
        origin: SpellOrigin::Caster,
        transition_ticks: TRANSITION_TICKS,
        stage_tint: super::stored_spells::stage_tint(scene.stage_color, ACTIVE_SCENE_TICKS)?,
        camera: super::stored_spells::scene_camera(scene.camera, ACTIVE_SCENE_TICKS)?,
        duration: TRANSITION_TICKS + CLEANUP_SCENE_TICK,
        startup: Some((
            TRANSITION_TICKS + MODEL_START_SCENE_TICK,
            SpellStartup::AllyRing {
                effects: [effect(2), effect(3), effect(4), effect(5)],
                first_heading: 15.,
                heading_step: 90.,
            },
        )),
        sound: None,
    })
}
