use anyhow::{Context, Result, bail};
use resonance_battle::{ActionRequest, Actor, Battle, BattleInput, ResourceId, Side};
use resonance_content::prepared::Files;
use resonance_game::battle::{self, ActionDefinition, ActionResources, EffectResource};
use std::sync::Arc;
mod common;

#[path = "battle_preparation/companion_normals.rs"]
mod companion_normals;

#[path = "battle_preparation/all_party_encounter.rs"]
mod all_party_encounter;

#[path = "battle_preparation/martial.rs"]
mod martial;

#[path = "battle_preparation/death.rs"]
mod death;
#[path = "battle_preparation/enemy_attack.rs"]
mod enemy_attack;
#[path = "battle_preparation/normal_attack.rs"]
mod normal_attack;
#[path = "battle_preparation/victory.rs"]
mod victory;
#[path = "battle_preparation/victory_selection.rs"]
mod victory_selection;
#[path = "battle_preparation/voice.rs"]
mod voice;

#[path = "battle_preparation/entry.rs"]
mod entry;

#[path = "battle_preparation/ex29.rs"]
mod ex29;

#[path = "battle_preparation/encounter.rs"]
mod encounter;

#[path = "battle_preparation/party.rs"]
mod party;

#[path = "battle_preparation/player_control.rs"]
mod player_control;

#[path = "battle_preparation/fire_ball.rs"]
mod fire_ball;
#[path = "battle_preparation/pattern_spell.rs"]
mod pattern_spell;
#[path = "battle_preparation/projectile_source.rs"]
mod projectile_source;

#[path = "battle_preparation/recoil.rs"]
mod recoil;

#[path = "battle_preparation/profile.rs"]
mod profile;

#[path = "battle_preparation/poison.rs"]
mod poison;

#[path = "battle_preparation/model.rs"]
mod model;

fn enter_battle(battle: &mut Battle) -> Result<()> {
    for _ in 0..600 {
        let frame = battle.step(BattleInput::default())?;
        if battle.phase() == resonance_battle::BattlePhase::Combat
            && !frame.clock.paused()
            && !battle.is_paused()
        {
            return Ok(());
        }
    }
    bail!("battle entry did not enable actor control")
}

fn assert_stored_spell_art(effect: &battle::encounter::RenderEffect, palette: u8) -> Result<()> {
    let mut tinted = false;
    for event in effect.source.program(50)? {
        if let resonance_content::battle_effect::EffectOperation::Spawn {
            particle, birth, ..
        } = &event.operation
        {
            let member = u16::from(*particle);
            assert!(
                effect.members.contains(&member),
                "missing stored-spell particle {member}"
            );
            let particle = effect.source.particle(usize::from(member))?;
            if particle.element_tint || birth.element_tint {
                tinted = true;
                assert!(
                    effect.palettes.get(&member).is_some_and(|pairs| {
                        pairs.contains(&[palette, particle.state.palettes[1]])
                    }),
                    "stored-spell particle {member} lacks elemental palette {palette}"
                );
            }
        }
    }
    assert!(tinted, "stored-spell program has no elemental particle");
    Ok(())
}

fn opening_setup(
    formation: u16,
    defeat: resonance_events::battle::DefeatPolicy,
) -> resonance_events::battle::Setup {
    resonance_events::battle::Setup {
        route: [0; 5],
        encounter: resonance_events::battle::Encounter::Formation(formation),
        arena: 13,
        defeat,
        music: None,
    }
}

fn opening_options(random_seed: u64) -> battle::encounter::PrepareOptions {
    battle::encounter::PrepareOptions {
        hunting_symbol: None,
        event_flags: Default::default(),
        devils_arms_unlocked: false,
        victory_story_flags: [false; 2],
        random_seed,
        map: 332,
        world_music: 0,
        story: 2500,
        colette_state: 0,
        overlimit_boost: false,
    }
}

fn prepare_party_fixture(
    files: &Files,
    menus: &resonance_content::menu_data::MenuData,
    party: &resonance_events::party::Party,
    setups: Vec<battle::party::Setup>,
) -> Result<Vec<battle::encounter::PreparedPartyMember>> {
    let session = resonance_content::session::SessionData::load(&files)?;
    let inputs = battle::encounter::Assets::capture(
        &mut (files)
            .library()
            .context("named library is required")?
            .snapshot(),
        menus,
        &session,
        party,
        opening_setup(1, resonance_events::battle::DefeatPolicy::ResumeEvent),
        (files).diagnostics().clone(),
        &(|| false),
    )?;
    let mut next_resource = ResourceId(1000);
    setups
        .into_iter()
        .enumerate()
        .map(|(slot, setup)| {
            let character = usize::from(party.formation[slot] - 1);
            let loadout = battle::party::loadout(
                menus,
                &party.members[character],
                battle::party::Character::from_index(character)?,
            )?;
            let member = inputs.prepare_party_member(
                menus,
                slot,
                loadout,
                setup,
                false,
                next_resource,
                None,
            )?;
            next_resource = member.next_resource;
            Ok(member)
        })
        .collect()
}

fn single_party_fixture(
    files: &Files,
    character: u8,
    weapon: u16,
    setup: battle::party::Setup,
) -> Result<battle::encounter::PreparedPartyMember> {
    let menus = resonance_content::menu_data::MenuData::load(&files)?;
    let data = resonance_content::session::SessionData::load(&files)?;
    let mut party = resonance_events::party::Party::new(&data, Default::default())?;
    party.formation = vec![character];
    let member = party
        .members
        .get_mut(usize::from(
            character.checked_sub(1).context("zero character")?,
        ))
        .context("missing fixture character")?;
    member.equipment = [weapon, 0, 0, 0, 0, 0];
    member.ex_skills = [0; 4];
    member.compound_ex_skills.clear();
    Ok(prepare_party_fixture(files, &menus, &party, vec![setup])?.remove(0))
}

fn prepare_enemy_fixture(
    files: &Files,
    enemy: u8,
    variant: usize,
    resource: u32,
) -> Result<battle::encounter::PreparedEnemyMember> {
    use resonance_content::battle_formation;
    let menus = resonance_content::menu_data::MenuData::load(&files)?;
    let data = resonance_content::session::SessionData::load(&files)?;
    let mut party = resonance_events::party::Party::new(&data, Default::default())?;
    party.formation = vec![1];
    let formations: battle_formation::Formations =
        resonance_content::battle_rules::formations(&files)?;
    let (encounter, spawn) = formations
        .records
        .iter()
        .enumerate()
        .find_map(|(encounter, row)| {
            row.actors
                .iter()
                .position(|spawn| {
                    row.resources[usize::from(spawn.resource)].enemy == u16::from(enemy)
                        && usize::from(spawn.variant) == variant
                        && spawn.unsupported_reason.is_none()
                })
                .map(|spawn| (encounter, spawn))
        })
        .context("no authored formation contains fixture enemy variant")?;
    let inputs = battle::encounter::Assets::capture(
        &mut (files)
            .library()
            .context("named library is required")?
            .snapshot(),
        &menus,
        &data,
        &party,
        opening_setup(
            u16::try_from(encounter)?,
            resonance_events::battle::DefeatPolicy::ResumeEvent,
        ),
        (files).diagnostics().clone(),
        &(|| false),
    )?;
    inputs.prepare_enemy_member(
        spawn,
        ResourceId(resource),
        ResourceId(2000),
        &mut resonance_battle::Random::new(1),
    )
}

fn actor() -> Actor {
    Actor {
        hp: 10,
        tp: 40,
        ..Actor::new(
            Side::Party,
            Default::default(),
            resonance_battle::EquipmentAttributes {
                normal_combo_limit: 3,
                ..resonance_battle::EquipmentAttributes::new(
                    100,
                    40,
                    resonance_battle::CombatStats::default(),
                )
            },
        )
    }
}

fn files() -> Files {
    let mut files = Files::default();
    files.insert(
        resonance_content::battle_model::document::PARTY.into(),
        serde_json::to_vec(&serde_json::json!({
            "1": "lloyd", "2": "colette", "3": "genis", "4": "raine", "5": "sheena",
            "6": "zelos", "7": "presea", "8": "regal", "9": "kratos"
        }))
        .unwrap()
        .into(),
    );
    files.insert(
        resonance_content::battle_recoil::PATH.into(),
        serde_json::to_vec(&recoil::source()).unwrap().into(),
    );
    let effects = resonance_content::battle_effect::document::Definition {
        artwork: None,
        programs: vec![Vec::new(); 29],
        templates: Vec::new(),
        template_names: Vec::new(),
    };
    files.insert(
        "test-effects.json".into(),
        serde_json::to_vec(&effects).unwrap().into(),
    );
    files.insert(
        resonance_content::battle_projectile::PATH.into(),
        serde_json::to_vec(&serde_json::json!({
            "version": resonance_content::battle_projectile::VERSION,
            "resident_spells": {}, "support_durations": {}, "fire_ball": null,
            "records": [{
                "lifetime": 20, "clamp_ground": true, "active": null,
                "ground_effect": {"bank": 0, "member": 0},
                "birth_effect": {"bank": 1, "member": 28},
                "trail_effect": {"bank": 1, "member": 0},
                "trail_interval": null, "shadow": null, "unsupported_reason": null,
                "velocity": [0., 0., 0.], "acceleration": [0., 0., 0.],
                "spawn_offset": [0., 0., 0.],
                "motion": {"velocity_jitter": [0., 0., 0.], "speed": null,
                           "steering": null, "response": null},
                "contact": {"damage_kind": "magic", "guarded": false, "recoil": 1,
                    "recoil_direction": "away_from_owner", "repeat_limit": 0,
                    "shape": {"kind": "cylinder"}, "radius": 10., "height": 20.,
                    "growth": [0., 0.], "offset": [0., 0., 0.],
                    "survives_contact": true, "clashes": false}
            }]
        }))
        .unwrap()
        .into(),
    );
    files
}
fn fixture_effect(files: &Files, members: Vec<u16>) -> Result<EffectResource> {
    Ok(EffectResource {
        bank: battle::effect_program::load(files, "test-effects.json")?,
        resource: ResourceId(37),
        members,
        models: Default::default(),
    })
}

fn fixture_hit() -> resonance_battle::HitRule {
    use resonance_battle::{DamageKind, HitElement, HitRule, Power};
    HitRule {
        sound: None,
        kind: DamageKind::Slash,
        arte: false,
        effect_on_guard: false,
        effect: None,
        overlimit_pause: false,
        element: HitElement::Neutral,
        power: Power::Fixed(5),
        prevents_defeat: false,
        finisher_grade: false,
        hp_drain: false,
        guard: Default::default(),
        reaction: Default::default(),
        condition: None,
    }
}

fn effects(
    frame: &resonance_battle::BattleFrame,
) -> impl Iterator<Item = &resonance_battle::EffectRequest> {
    frame.cues.iter().filter_map(|cue| match cue {
        resonance_battle::Cue::Effect(request)
        | resonance_battle::Cue::InitializedEffect(request) => Some(request),
        _ => None,
    })
}

#[path = "battle_preparation/escape.rs"]
mod escape;

#[path = "battle_preparation/lethal_rescue.rs"]
mod lethal_rescue;
