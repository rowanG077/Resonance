use anyhow::{Context, Result, bail};
use resonance_battle::{ActionRequest, Actor, Battle, BattleInput, Side};
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

fn prepare_party_fixture(
    files: &Files,
    menus: &resonance_content::menu_data::MenuData,
    party: &resonance_events::party::Party,
    setups: Vec<battle::party::Setup>,
) -> Result<Vec<battle::encounter::PreparedPartyMember>> {
    let session = files.json("game/session-data.json")?;
    let inputs = battle::encounter::Inputs::load(
        &common::asset_root(),
        files,
        menus,
        &session,
        party,
        resonance_events::battle::Setup {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(1),
            arena: 13,
            defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
            music: None,
        },
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let mut next_resource = 1000;
    setups
        .into_iter()
        .enumerate()
        .map(|(slot, setup)| {
            let character = usize::from(party.formation[slot] - 1);
            let loadout = battle::party::loadout(menus, &party.members[character], character)?;
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
    let menus = files.json("game/menu-data.json")?;
    let data = files.json("game/session-data.json")?;
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
    let menus = files.json("game/menu-data.json")?;
    let data = files.json("game/session-data.json")?;
    let mut party = resonance_events::party::Party::new(&data, Default::default())?;
    party.formation = vec![1];
    let formations: battle_formation::Formations = files.json(battle_formation::PATH)?;
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
    let inputs = battle::encounter::Inputs::load(
        &common::asset_root(),
        files,
        &menus,
        &data,
        &party,
        resonance_events::battle::Setup {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(u16::try_from(encounter)?),
            arena: 13,
            defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
            music: None,
        },
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    inputs.prepare_enemy_member(spawn, resource, 2000, &mut resonance_battle::Random::new(1))
}

fn actor() -> Actor {
    Actor {
        side: Side::Party,
        species: 0,
        equipment: resonance_battle::EquipmentAttributes {
            max_hp: 100,
            max_tp: 40,
            tp_cost_reduction: false,
            quick_escape: false,
            taunt_enabled: false,
            taunt_guard: false,
            taunt_cancel: false,
            control_ex: Default::default(),
            quick_turn: false,
            backstep_guard: false,
            casting: Default::default(),
            dagger_reach: false,
            contact: Default::default(),
            normal_combo_limit: 3,
            luck: 0,
            stats: resonance_battle::CombatStats::default(),
            affinities: [resonance_battle::Affinity::Normal; 9],
            damage: Default::default(),
            recovery: Default::default(),
            base_element: None,
            combo_traits: Default::default(),
            normal_guard: false,
            speed_multiplier: 1.,
            reaction_ex: Default::default(),
            stun_ex_bonus: false,
            spell_revenge: false,
        },
        control: Default::default(),
        availability: Default::default(),
        overlimit: Default::default(),
        proficiency: 0,
        input: Default::default(),
        guard: Default::default(),
        hp: 10,
        tp: 40,
        control_ex_state: Default::default(),
        casting_state: Default::default(),
        stored_spell: None,
        control_slot: 0,
        elements: Default::default(),
        attack_power: 100,
        conditions: Default::default(),
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        movement: Default::default(),
        hit_stop: 0,
        time_stop: 0,
        reaction: Default::default(),
        body: resonance_battle::Body::default(),
    }
}

fn files() -> Files {
    let mut files = Files::default();
    files.insert(
        resonance_content::battle_recoil::PATH.into(),
        serde_json::to_vec(&recoil::source()).unwrap().into(),
    );
    let effects = resonance_content::battle_effect::SourceBank {
        art: None,
        source_sha256: "a".repeat(64),
        programs: vec![Vec::new(); 29],
        actors: vec![],
    };
    files.insert(
        "test-effects.json".into(),
        serde_json::to_vec(&effects).unwrap().into(),
    );
    files.insert(
        "test-projectiles.json".into(),
        serde_json::to_vec(&serde_json::json!({
            "source_sha256": "a".repeat(64),
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
                    "direction": "away_from_owner", "repeat_limit": 0,
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
        resource: 37,
        members,
        models: Default::default(),
    })
}

fn fixture_hit() -> resonance_battle::HitRule {
    use resonance_battle::{DamageKind, HitElement, HitRule, Power};
    HitRule {
        kind: DamageKind::Slash,
        arte: false,
        overlimit_pause: false,
        element: HitElement::Neutral,
        power: Power::Fixed(5),
        prevents_defeat: false,
        guard: Default::default(),
        reaction: Default::default(),
        condition: None,
    }
}

fn effects(
    frame: &resonance_battle::BattleFrame,
) -> impl Iterator<Item = &resonance_battle::EffectRequest> {
    frame.cues.iter().filter_map(|cue| match cue {
        resonance_battle::Cue::Effect(request) => Some(request),
        _ => None,
    })
}

#[path = "battle_preparation/escape.rs"]
mod escape;

#[path = "battle_preparation/lethal_rescue.rs"]
mod lethal_rescue;
