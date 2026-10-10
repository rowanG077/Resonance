//! Read-only observations for the existing checkpoint recorder and screenshot sidecars.
use super::{Owner, Phase, SceneState};
use resonance_battle::{ActorId, BattlePhase, BattleResult, Control, Side};
use resonance_game::battle::command::{ActorSelection, View};
use serde_json::{Value, json};

fn model_material(material: resonance_battle::ModelMaterial) -> &'static str {
    match material {
        resonance_battle::ModelMaterial::Normal => "normal",
        resonance_battle::ModelMaterial::RedChannel => "red_channel",
    }
}

fn main_motion(motion: resonance_battle::MainMotionObservation) -> Value {
    json!({
        "clip": motion.clip, "frame": motion.frame,
        "end": motion.end, "loop_start": motion.loop_start, "rate": motion.rate,
        "repeat": motion.repeat, "stopped": motion.stopped, "finished": motion.finished,
        // Keep exact float evidence even if a diagnostic encounters a nonfinite
        // value that JSON cannot represent as an ordinary number.
        "float_bits": {"frame": motion.frame.to_bits(),
            "end": motion.end.to_bits(), "loop_start": motion.loop_start.to_bits(),
            "rate": motion.rate.to_bits()},
    })
}

fn actor_selection(selection: &ActorSelection) -> Value {
    json!({"actor": selection.actor.index(), "slot": selection.slot,
        "name": selection.name, "eligible": selection.eligible})
}

/// Describe the retained draw sample, which can differ from the next input owner
/// on the inventory's closing visit. Observation never advances either owner.
fn command_view(view: &View) -> Value {
    match view {
        View::Strip => json!({"kind": "strip"}),
        View::TechTarget { target } => {
            json!({"kind": "tech_target", "target": target.map(|id| id.index())})
        }
        View::User(selection) => {
            json!({"kind": "user", "selection": actor_selection(selection)})
        }
        View::Inventory(list) => json!({"kind": "inventory",
            "rows": list.rows.iter().map(|row| json!({"id": row.id, "count": row.count,
                "recent": row.recent, "urgent": row.urgent})).collect::<Vec<_>>(),
            "selected": list.selected, "first": list.first, "scroll": list.scroll,
            "fade": list.fade, "description_previous": list.description_previous,
            "description_blend": list.description_blend}),
        View::Tech(state) => json!({"kind": "tech", "state": state}),
        View::Strategy(state) => json!({"kind": "strategy", "state": state}),
        View::Unison(state) => json!({"kind": "unison", "state": state}),
        View::Equipment(state) => json!({"kind": "equipment", "state": state}),
        View::Ally(selection) => {
            json!({"kind": "ally", "selection": actor_selection(selection)})
        }
        View::Enemy { target } => json!({"kind": "enemy", "target": target.index()}),
    }
}

impl Owner {
    pub(crate) fn replay_phase(&self) -> Option<(u16, BattlePhase)> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        (self.presenting() && scene.entry_remaining == 0)
            .then_some((self.request.setup.formation().ok()?, scene.core.phase()))
    }

    /// Screenshot readback starts only after the scene is fully submitted.
    pub(crate) fn capture_ready(&self) -> bool {
        self.presenting()
    }

    pub(crate) fn entry_clock(&self) -> Option<(u64, u8)> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        Some((self.request.id(), scene.entry_remaining))
    }

    pub(crate) fn entry_diagnostic(&self) -> Option<Value> {
        let mut result = json!({"request": self.request.id(), "presenting": self.presenting()});
        if let Phase::Scene(scene) = &self.phase {
            result["state"] = json!(match scene.state {
                SceneState::Warming => "warming",
                SceneState::Activating => "activating",
                SceneState::Active => "active",
            });
            result["remaining_ticks"] = json!(scene.entry_remaining);
            result["active"] = json!(scene.entry_remaining > 0);
            result["alpha"] = json!(scene.entry_alpha());
        }
        Some(result)
    }

    pub(crate) fn capture_clock(&self) -> Option<(u64, u64, u32)> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        let frame = scene.frame.as_ref()?;
        (scene.state == SceneState::Active).then_some((
            self.request.id(),
            frame.update,
            self.input_tick,
        ))
    }

    /// None while loading or after failure: preparation is not a simulated frame.
    pub(crate) fn diagnostic(&self) -> Option<Value> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        if scene.state != SceneState::Active || scene.failure.is_some() {
            return None;
        }
        let frame = scene.frame.as_ref()?;
        let ledger = scene.core.ledger();
        let phase = match scene.core.phase() {
            BattlePhase::Entry => "entry",
            BattlePhase::Combat => "combat",
            BattlePhase::Ending => "ending",
            BattlePhase::Results => "results",
            BattlePhase::Finished => "finished",
        };
        let result = |value| match value {
            BattleResult::Victory => "victory",
            BattleResult::Defeat => "defeat",
            BattleResult::Escaped => "escaped",
        };
        let target_hold_counts: Vec<_> = scene.core.target_hold_counts().collect();
        let strategy_party = scene.candidate.as_ref().map(|candidate| {
            // Only the borrowed Party and prepared preset defaults are read;
            // this local page pose is never stepped or used as a draw sample.
            let state = resonance_game::menu::strategy::Strategy::default();
            let page = candidate.strategy_page(&state);
            let presets = page.presets();
            json!({
                "personal": page.party.members.iter().map(|member| member.strategy).collect::<Vec<_>>(),
                "presets": presets.as_ref().ok(),
                "preset_error": presets.as_ref().err().map(ToString::to_string),
            })
        });
        let actors: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                let hud = json!({
                "cast_delayed": matches!(actor.activity, resonance_battle::Activity::Casting { held: true }),
                "stored_spell": actor.stored_spell.map(|action| action.0)});
                let portrait_draw = (actor.side == Side::Party).then(|| {
                    let observed = scene.hud.portrait_observation(actor, frame.recognized_result);
                    json!({"row": observed.row, "color": observed.color})
                });
                json!({
                    "actor": index, "side": if actor.side == Side::Party { 0 } else { 1 },
                    "character": scene.characters.get(index),
                    "companion_policy": ActorId::from_index(index).ok()
                        .and_then(|actor| scene.core.companion_policy(actor))
                        .map(|policy| json!({"raw": policy.choices})),
                    "control": match actor.control {
                        Control::Manual => "manual", Control::SemiAuto => "semi_auto",
                        Control::Auto => "auto", Control::Enemy => "enemy",
                    },
                    "activity": format!("{:?}", actor.activity),
                    "casting": ActorId::from_index(index).ok()
                        .and_then(|actor| scene.core.casting_remaining(actor))
                        .map(|remaining| json!({"remaining": remaining})),
                    "overlimit_active": actor.overlimit.is_active(),
                    "availability": format!("{:?}", actor.availability),
                    "conditions": {
                        "base": actor.conditions.base(),
                        "intrinsic": actor.conditions.layers().intrinsic,
                        "equipment_overlay": actor.conditions.layers().equipment_overlay,
                        "immunity": actor.conditions.immunity(),
                        "effective": actor.conditions.effective(),
                        "active_effects": actor.conditions.active_effects().iter().map(|effect| json!({
                            "condition": effect.condition, "remaining": effect.remaining,
                            "magnitude": effect.magnitude,
                        })).collect::<Vec<_>>(),
                        "periodic_effects": actor.conditions.periodic_effects().iter().map(|effect| json!({
                            "condition": effect.condition, "remaining": effect.remaining,
                            "period": effect.period,
                        })).collect::<Vec<_>>(),
                    },
                    "elements": {
                        "action": actor.elements.action.map_or(0, |value| value as u8 + 1),
                        "enchantment": actor.elements.enchantment.map_or(0, |value| value as u8 + 1),
                        "base": actor.equipment.base_element.map_or(0, |value| value as u8 + 1),
                    },
                    "guard": format!("{:?}", actor.guard),
                    "hp": actor.hp, "max_hp": actor.equipment.max_hp, "tp": actor.tp, "max_tp": actor.equipment.max_tp,
                    "position": actor.position, "heading": actor.heading,
                    "center": actor.effect_origin(), "center_offset": actor.body.center_offset,
                    "target_center": actor.target_center(), "model_scale": actor.body.scale,
                    "target_hold_ticks": target_hold_counts.get(index).copied().flatten(),
                    "target": frame.targets.get(index).copied().flatten().map(|id| id.index()),
                    "hit_stop": actor.hit_stop, "overlimit_charge": actor.overlimit.charge(), "overlimit_remaining": actor.overlimit.remaining(), "hud": hud,
                    "portrait_draw": portrait_draw,
                })
            })
            .collect();
        let models: Vec<_> = frame
            .models
            .iter()
            .map(|model| {
                json!({
                    "actor": model.actor.index(), "resource": model.resource,
                    "visible": model.visible, "clip": model.clip, "frame": model.frame,
                    // Live post-callback flag; the pose above can be held from an
                    // earlier drawing boundary. Terminal stop uses this flag too.
                    "current_main": scene.models.main_motion_observation(model.actor).map(main_motion),
                    "blend_weight": model.blend_weight,
                    "root_translation": model.root_translation,
                    "world": model.world, "bones": model.bones.as_ref(),
                    "tint": model.tint, "material": model_material(model.material),
                    "outline_tint": super::appearance::outline(&frame.actors[model.actor.index()], frame.recognized_result.is_none(), model.tint[3]), "texture_layers": model.texture_layers,
                    "light": model.light,
                    "shadow": model.shadow.map(|shadow| json!({
                        "position": shadow.position, "radius": shadow.radius,
                        "color": shadow.color,
                    })),
                })
            })
            .collect();
        let mut diagnostic = json!({
            "request": self.request.id(), "encounter": self.request.setup.encounter,
            "arena": self.request.setup.arena, "phase": phase,
            "update": frame.update, "input_tick": self.input_tick,
            "gameplay_tick": ledger.elapsed_ticks, "combat_tick": ledger.combat_ticks,
            "random_state": {"entry_seed": scene.entry_seed, "state": scene.core.random_state()},
            "recognized_result": frame.recognized_result.map(result),
            "item_cooldown": frame.item_cooldown,
            "scanned_enemies": frame.scanned_enemies,
            "target_selector": frame.target_selector.map(|id| id.index()),
            "command": scene.lifecycle.command_frame().map(|command| json!({
                "actor": command.actor.index(), "selected": command.selected.index(),
                "enabled": command.enabled,
                "controller": command.controller,
                "view": command_view(&command.view),
            })),
            "strategy_party": strategy_party,
            "actors": actors,
            "models": models,
            "weapons": frame.weapons.iter().map(|weapon| json!({
                "actor": weapon.owner.index(), "slot": weapon.slot,
                "resource": weapon.resource, "visible": weapon.visible,
                "clip": weapon.clip, "frame": weapon.frame,
                "world": weapon.world, "bones": weapon.bones.as_ref(),
                "tint": weapon.tint, "material": model_material(weapon.material), "links": weapon.links,
            })).collect::<Vec<_>>(),
            "camera": frame.camera.map(|camera| json!({
                "eye": camera.eye, "target": camera.focus,
                "pitch": camera.pitch, "yaw": camera.yaw, "radius": camera.radius,
            })),
            "actions": frame.actions.iter().map(|(id, actor, age)| json!({
                "handle": format!("{id:?}"), "actor": actor.index(), "age": age,
                "recovery_remaining": scene.core.action_recovery_remaining(*id),
            })).collect::<Vec<_>>(),
            "particles": scene.particles.iter().map(|particle| json!({
                "handle": format!("{:?}", particle.id), "actor": particle.owner.index(),
                "resource": particle.resource, "member": particle.member,
                "origin": particle.origin, "offset": particle.state.offset,
            })).collect::<Vec<_>>(),
            "projectile_shadows": frame.projectile_shadows.iter().map(|shadow| json!({
                "handle": format!("{:?}", shadow.projectile), "position": shadow.position,
                "radius": shadow.appearance.radius, "color": shadow.appearance.color,
                "additive": shadow.appearance.additive,
            })).collect::<Vec<_>>(),
            "cues": frame.cues.iter().map(|cue| format!("{cue:?}")).collect::<Vec<_>>(),
            "initial_music": scene.music,
        });
        // Keep these read-only blocks outside the large frame object so each
        // JSON macro expansion stays within the ordinary recursion limit.
        diagnostic["controller_motors"] = json!(scene.feedback.motors(scene.feedback_paused()));
        diagnostic["pause"] = json!({
            "clock": format!("{:?}", frame.clock),
            "paused": frame.clock.paused(),
            "timed_hold_remaining": scene.core.timed_hold_remaining(),
        });
        diagnostic["overlays"] = scene.hud.overlays_diagnostic();
        diagnostic["pending_item"] = json!(scene.core.pending_item().map(|item| json!({
            "user": item.user.index(), "target": item.target.index(), "item": item.item,
        })));
        // The release removes exhausted stacks; an absent item key is zero.
        diagnostic["inventory"] = json!(
            scene
                .candidate
                .as_ref()
                .map(|candidate| candidate.items().counts)
        );
        // This is the candidate's actual persistent snapshot, not an export
        // synthesized by the recorder from live or retained actor/model state.
        // The field's separately captured persistent_party remains authoritative
        // after normal commit; these values can legitimately lag during battle.
        diagnostic["candidate_party"] = json!(scene.candidate.as_ref().map(|candidate| {
            let party = candidate.persistent_party();
            json!({
                "formation": party.formation,
                "members": party.members.iter().enumerate().map(|(index, member)| json!({
                    "character": index + 1,
                    "hp": member.hp, "tp": member.tp,
                    "ailments": member.ailments, "queued_buffs": member.queued_buffs,
                })).collect::<Vec<_>>(),
            })
        }));
        // Result callbacks may mutate vitals after frame sampling. Keep these
        // post-callback observations separate and leave combat sidecars intact.
        if scene.core.phase() == BattlePhase::Results
            && let Some(pending) = scene.candidate.as_ref().and_then(|c| c.pending_results())
        {
            use resonance_game::battle::results::ResultNotice;
            let notices: Vec<_> = pending.results.notices.iter().map(|notice| match notice {
                ResultNotice::Level { character, level } => json!({"kind": "level", "character": character, "level": level}),
                ResultNotice::TpRecovery { character, amount } => json!({"kind": "tp_recovery", "character": character, "amount": amount}),
                ResultNotice::MaximumVital { character, vital, amount } =>
                    json!({"kind": "maximum_vital", "character": character,
                        "vital": format!("{vital:?}"), "amount": amount}),
                ResultNotice::HappinessExperience { character, amount } =>
                    json!({"kind": "happiness_experience", "character": character, "amount": amount}),
                ResultNotice::HappinessGald { character, amount } =>
                    json!({"kind": "happiness_gald", "character": character, "amount": amount}),
                ResultNotice::Technique { character, technique } =>
                    json!({"kind": "technique", "character": character, "technique": technique}),
                ResultNotice::CompoundEx { character } =>
                    json!({"kind": "compound_ex", "character": character}),
                ResultNotice::Title { character, title } =>
                    json!({"kind": "title", "character": character, "title": title}),
                ResultNotice::Cooking { character, recipe, success } =>
                    json!({"kind": "cooking", "character": character, "recipe": recipe, "success": success}),
            }).collect();
            diagnostic["results"] = json!({
                "pending_party": pending.party,
                "gameplay_random": pending.gameplay_random,
                "accepted": pending.accepted,
                "cook_prompt": pending.results.cook_prompt,
                "notices": notices,
                "cards": scene.hud.results_diagnostic(),
                "live_actors": scene.core.actors().iter().enumerate()
                    .filter(|(_, actor)| actor.side == Side::Party)
                    .map(|(index, actor)| json!({
                        "actor": index, "character": scene.characters.get(index),
                        "hp": actor.hp, "max_hp": actor.equipment.max_hp,
                        "tp": actor.tp, "max_tp": actor.equipment.max_tp,
                        "overlimit_charge": actor.overlimit.charge(), "overlimit_remaining": actor.overlimit.remaining(),
                        "availability": format!("{:?}", actor.availability),
                    })).collect::<Vec<_>>(),
            });
        }
        Some(diagnostic)
    }
}
