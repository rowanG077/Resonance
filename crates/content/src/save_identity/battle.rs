//! Gameplay projection of current battle documents. Paths and presentation
//! choices are resolved or removed before recording compatibility inputs.
use super::{record, record_json};
use crate::{
    battle_action as action, battle_normal::Event, library::Snapshot, preparation::JSON_LIMIT,
};
use anyhow::{Context, Result, ensure};
use sha2::Sha256;

mod effects;
mod models;
#[cfg(test)]
mod tests;

pub(super) fn capture(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    // Small authored libraries can omit battle entirely. Once battle rules are
    // present, all shared rules and mapped gameplay resources are required.
    if snapshot
        .read_optional(crate::battle_rules::PARTY, JSON_LIMIT)?
        .is_none()
    {
        return Ok(());
    }
    let mut rules = crate::battle_rules::capture(snapshot, cancelled)?;
    for formation in &mut rules.formations.records {
        formation.settings.play_music = false;
        formation.settings.entry_voice = false;
        for resource in &mut formation.resources {
            resource.hidden_name = false;
        }
        for actor in &mut formation.actors {
            actor.texture_variant = 0;
        }
    }
    record_json(hash, "battle/formations", &rules.formations)?;
    for attacks in &mut rules.party.normal_attacks {
        for attack in attacks {
            normal_events(&mut attack.events);
        }
    }
    for member in rules.party.martial_attacks.members.values_mut() {
        for variant in member.variants.values_mut() {
            scene_action(variant);
        }
    }
    let profiles = rules
        .party
        .records
        .iter()
        .map(profile)
        .collect::<Result<Vec<_>>>()?;
    record_json(
        hash,
        "battle/party",
        &(
            rules.party.body_colliders,
            rules.party.default_strategy,
            rules.party.companion_policy,
            rules.party.normal_attacks,
            rules.party.martial_attacks,
            rules.party.chant_sequences,
            profiles,
        ),
    )?;
    record_json(
        hash,
        "battle/defense-system",
        &(
            rules.defense.core,
            rules.defense.repair,
            rules.defense.orbit,
            rules.defense.orbit_count,
            rules.defense.arm,
            rules.defense.arm_slots,
            rules.defense.deployment,
            rules.defense.repair_cycle,
        ),
    )?;
    for opener in &mut rules.unison.openers {
        normal_events(&mut opener.events);
    }
    for combination in &mut rules.unison.combinations {
        combination.name.clear();
        combination.camera_pitch = 0.;
    }
    let host = rules.unison.host;
    record_json(
        hash,
        "battle/unison",
        &(
            rules.unison.openers,
            rules.unison.combinations,
            rules.unison.placements,
            host.placement,
            host.minimum_distance,
            host.short_weapon_penalty,
            host.restore_height,
            host.windup_rate,
            host.prelude.first_x,
            host.prelude.spacing,
        ),
    )?;
    for projectile in &mut rules.projectiles.records {
        projectile.shadow = None;
    }
    for spell in rules.projectiles.resident_spells.values_mut() {
        hit(&mut spell.hit);
        if let Some(value) = &mut spell.followup_hit {
            hit(value);
        }
    }
    record_json(hash, "battle/projectiles", &rules.projectiles)?;
    record_json(hash, "battle/recoil", &rules.recoil)?;
    for technique in &mut rules.techniques.definitions {
        technique.admission_flash = None;
        technique.casting.effects = crate::arte::CastingEffects::Standard;
    }
    record_json(hash, "battle/techniques", &rules.techniques)?;
    for (name, path) in [
        (
            "battle/common-effects",
            crate::battle_effect::document::COMMON,
        ),
        (
            "battle/technique-effects",
            crate::battle_effect::document::TECHNIQUES,
        ),
    ] {
        effects::capture(snapshot, hash, name, path, cancelled)?;
    }

    for id in ids(snapshot, crate::battle_enemy::document::MAPPINGS)? {
        ensure!(!cancelled(), "battle identity preparation cancelled");
        use crate::battle_enemy::document as enemy;
        let path = snapshot.resolve(enemy::MAPPINGS, id, Some("enemy.json"))?;
        let entry: enemy::Entry = snapshot.json(&path, JSON_LIMIT)?;
        version(entry.version, enemy::VERSION, &path)?;
        let mut rules: enemy::Rules = snapshot.json(&entry.rules, JSON_LIMIT)?;
        version(rules.version, enemy::VERSION, &entry.rules)?;
        for row in &mut rules.actions.rows {
            match &mut row.execution {
                Some(
                    action::EnemyExecution::Attack(attack)
                    | action::EnemyExecution::Physical { attack, .. },
                ) => {
                    enemy_events(&mut attack.events);
                    if let Some(recovery) = &mut attack.contact_recovery {
                        recovery.voice = None;
                        recovery.expressions = [0; 4];
                    }
                }
                _ => (),
            }
        }
        for projectile in &mut rules.projectiles {
            projectile.definition.shadow = None;
        }
        record_json(
            hash,
            &format!("battle/enemy/{id}"),
            &(
                &rules.null_motions,
                profile(&rules.profile)?,
                &rules.weapon_visibility,
                &rules.projectiles,
                &rules.actions,
                &rules.target_strategy,
                &rules.entry_row,
                rules.guard_recovery_bonus,
            ),
        )?;
        if let Some(crate::battle_enemy::Opening::Entrance(entrance)) = rules
            .presentation
            .as_ref()
            .and_then(|presentation| presentation.opening.as_ref())
        {
            use crate::battle_enemy::EntranceCue as Cue;
            let movement: Vec<_> = entrance
                .cues
                .iter()
                .filter(|(_, cue)| {
                    matches!(
                        cue,
                        Cue::Height(_)
                            | Cue::Vertical(_)
                            | Cue::Pose { .. }
                            | Cue::PartyPose { .. }
                    )
                })
                .collect();
            record_json(
                hash,
                &format!("battle/enemy/{id}/entrance"),
                &(
                    entrance.ticks,
                    entrance.gravity,
                    &entrance.launches,
                    movement,
                ),
            )?;
        }
        models::enemy(snapshot, hash, id, &entry.artwork, cancelled)?;
        if let Some(path) = entry.effects {
            effects::capture(
                snapshot,
                hash,
                &format!("battle/enemy/{id}/effects"),
                &path,
                cancelled,
            )?;
        }
    }
    for mapping in [
        crate::battle_scene::document::TECHNIQUES,
        crate::battle_scene::document::SKILLS,
    ] {
        for id in ids(snapshot, mapping)? {
            ensure!(!cancelled(), "battle identity preparation cancelled");
            use crate::battle_scene::document as scene;
            let path = snapshot.resolve(mapping, id, Some("scene.json"))?;
            let entry: scene::Definition = snapshot.json(&path, JSON_LIMIT)?;
            version(entry.version, scene::VERSION, &path)?;
            let mut rules: scene::Rules = snapshot.json(&entry.rules, JSON_LIMIT)?;
            version(rules.version, scene::VERSION, &entry.rules)?;
            rules.stage_color = None;
            rules.camera = None;
            for action in rules.actions.values_mut() {
                scene_action(action);
            }
            for projectile in &mut rules.projectiles {
                projectile.projectile.shadow = None;
                if let Some(value) = &mut projectile.hit {
                    hit(value);
                }
                for value in projectile.hits.iter_mut().flatten() {
                    hit(value);
                }
            }
            let name = format!("{mapping}/{id}");
            record_json(hash, &name, &rules)?;
            if let Some(weapon) = entry.weapon {
                models::weapon(snapshot, hash, &name, weapon, cancelled)?;
            }
            effects::capture(snapshot, hash, &name, &entry.effects, cancelled)?;
        }
    }
    for id in ids(snapshot, crate::battle_event::document::MAPPINGS)? {
        ensure!(!cancelled(), "battle identity preparation cancelled");
        use crate::battle_event::document as event;
        let path = snapshot.resolve(event::MAPPINGS, id, Some("event.json"))?;
        let entry: event::Definition = snapshot.json(&path, JSON_LIMIT)?;
        version(entry.version, event::VERSION, &path)?;
        record(
            hash,
            &format!("battle/event/{id}"),
            &snapshot.read(&entry.script, JSON_LIMIT)?,
        );
        for (actor, path) in entry.actors {
            let model: event::Actor = snapshot.json(&path, JSON_LIMIT)?;
            version(model.version, event::VERSION, &path)?;
            let name = format!("battle/event/{id}/actor/{actor}");
            record_json(hash, &format!("{name}/skeleton"), &model.skeleton)?;
            models::parts(
                snapshot,
                hash,
                &name,
                std::slice::from_ref(
                    model
                        .parts
                        .first()
                        .context("battle event actor has no body")?,
                ),
                cancelled,
            )?;
        }
    }
    models::party_and_equipment(snapshot, hash, cancelled)?;
    Ok(())
}

fn ids(snapshot: &mut Snapshot, mapping: &str) -> Result<Vec<u32>> {
    Ok(snapshot
        .mappings(mapping)?
        .iter()
        .map(|(id, _)| id)
        .collect())
}

fn version(actual: u32, expected: u32, path: &str) -> Result<()> {
    ensure!(
        actual == expected,
        "unsupported gameplay document version in {path}"
    );
    Ok(())
}

fn profile(value: &crate::battle_profile::Profile) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(value)?;
    let fields = value.as_object_mut().expect("profile is an object");
    for name in [
        "body_alpha",
        "texture_channels",
        "blink",
        "idle_expression",
        "active_expression",
        "rescue_expression",
        "weapon_styles",
        "shadow_scale",
        "shadow_color",
        "camera_category",
        "voices",
        "head_anchor",
        "overlimit_bone",
        "overlimit_scale",
    ] {
        fields.remove(name);
    }
    // Shadow visibility does not affect body placement or reception geometry.
    fields
        .get_mut("traits")
        .and_then(serde_json::Value::as_object_mut)
        .expect("profile traits are an object")
        .remove("hide_enemy_shadow");
    Ok(value)
}

fn hit(hit: &mut action::ActionHit) {
    hit.effect = None;
    hit.effect_on_guard = false;
    hit.suppress_guard_sound = false;
    hit.sound = None;
}

fn normal_events(events: &mut Vec<(u16, Event)>) {
    events.retain_mut(|(_, event)| {
        match event {
            Event::Sound(_)
            | Event::Voice(_)
            | Event::Trail { .. }
            | Event::CameraDistance { .. } => return false,
            Event::Contact(contact) => hit(&mut contact.hit),
            Event::Throw { hit: value, .. } => hit(value),
            Event::Projectile {
                hit: value,
                feedback,
                ..
            } => {
                if let Some(value) = value {
                    hit(value);
                }
                *feedback = None;
            }
            _ => (),
        }
        true
    });
}

fn scene_action(action: &mut crate::battle_scene::SceneAction) {
    let crate::battle_scene::SceneAction::Prepared(attack) = action else {
        return;
    };
    normal_events(&mut attack.events);
    for value in attack.callback_hits.values_mut() {
        hit(value);
    }
    if let Some(hammer) = &mut attack.hammer {
        for value in &mut hammer.hits {
            hit(value);
        }
        hammer.impact_sound = 0;
        hammer.impact_effect = 0;
        match &mut hammer.pattern {
            crate::battle_martial::HammerPattern::Single { alternate_events } => {
                normal_events(alternate_events)
            }
            crate::battle_martial::HammerPattern::Ring { opening_hit, .. } => hit(opening_hit),
            _ => (),
        }
    }
    if let Some(steal) = &mut attack.steal {
        normal_events(&mut steal.success_events);
        hit(&mut steal.bonus_hit);
    }
    if let Some(fang) = &mut attack.fierce_fang {
        for value in &mut fang.hits {
            hit(value);
        }
    }
    if let Some(value) = &mut attack.hurricane_hit {
        hit(value);
    }
}

fn enemy_events(events: &mut Vec<(u16, action::EnemyEvent)>) {
    use action::EnemyEvent as E;
    events.retain_mut(|(_, event)| {
        match event {
            E::Sound(_) | E::Voice { .. } | E::Trail { .. } | E::Tint(_) => return false,
            E::Contact(contact) | E::ContinueContact(contact) => hit(&mut contact.hit),
            E::Projectile {
                definition,
                hit: value,
                feedback,
            } => {
                definition.shadow = None;
                hit(value);
                *feedback = None;
            }
            E::GroundProjectiles {
                definitions,
                hit: value,
                ..
            } => {
                for definition in definitions.iter_mut() {
                    definition.shadow = None;
                }
                hit(value);
            }
            _ => (),
        }
        true
    });
}
