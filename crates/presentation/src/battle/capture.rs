//! Read-only observations for the existing checkpoint recorder and screenshot sidecars.
use super::{Owner, Phase};
use resonance_battle::{BattlePhase, BattleResult, Control, Side};
use serde_json::{Value, json};

impl Owner {
    /// The live entry can be presented while the battle GPU resources warm.
    /// A held screenshot must first finish those submissions, otherwise its
    /// target/readiness diagnostics can change during an asynchronous readback.
    /// The recorder waits with zero elapsed time, preserving all entry clocks.
    pub(crate) fn capture_ready(&self) -> bool {
        self.presenting() && matches!(&self.phase, Phase::Scene(scene) if scene.gpu_ready)
    }

    pub(crate) fn entry_clock(&self) -> Option<(u64, u16)> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        Some((self.request.id(), scene.transition.timer()))
    }

    pub(crate) fn entry_diagnostic(&self) -> Option<Value> {
        let capture = self.entry.as_ref()?.diagnostic();
        let mut result = json!({"request": self.request.id(), "capture": capture,
            "presenting": self.presenting()});
        if let Phase::Scene(scene) = &self.phase {
            result["dispatch"] =
                json!({"name":scene.dispatch.name(), "source":scene.dispatch.source()});
            result["timer"] = json!(scene.transition.timer());
            result["active"] = json!(scene.transition.active());
            result["alpha"] = json!(scene.transition.alpha());
            result["fade"] = json!(
                scene
                    .transition
                    .fade()
                    .map(|fade| { json!({"color": fade.color, "alpha": fade.alpha}) })
            );
            result["battle_gpu_ready"] = json!(scene.gpu_ready);
            result["battle_core_active"] = json!(scene.active);
        }
        Some(result)
    }

    pub(crate) fn capture_combat(&self) -> Option<(u64, u16, u32)> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        (scene.active && scene.failure.is_none()).then_some((
            self.request.id(),
            self.request.setup.encounter,
            scene.core.ledger().combat_ticks,
        ))
    }

    pub(crate) fn capture_clock(&self) -> Option<(u64, u64, u32)> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        let frame = scene.frame.as_ref()?;
        scene
            .active
            .then_some((self.request.id(), frame.update, frame.hud_update))
    }

    /// None while loading or after failure: preparation is not a simulated frame.
    pub(crate) fn diagnostic(&self) -> Option<Value> {
        let Phase::Scene(scene) = &self.phase else {
            return None;
        };
        if !scene.active || scene.failure.is_some() {
            return None;
        }
        let frame = scene.frame.as_ref()?;
        let ledger = scene.core.ledger();
        let phase = if scene.core.entry_pending() {
            "entry"
        } else {
            match scene.core.phase() {
                BattlePhase::Combat => "combat",
                BattlePhase::Ending => "ending",
                BattlePhase::Results => "results",
                BattlePhase::Finished => "finished",
            }
        };
        let result = |value| match value {
            BattleResult::Victory => "victory",
            BattleResult::Defeat => "defeat",
            BattleResult::Escaped => "escaped",
        };
        let target_hold_counts: Vec<_> = scene.core.target_hold_counts().collect();
        let actors: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                let hud = json!({"hp": actor.hud.hp, "tp": actor.hud.tp, "phase": actor.hud.phase,
                "hp_trail": actor.hud.hp_trail, "tp_trail": actor.hud.tp_trail,
                "portrait_bounce": actor.hud.portrait_bounce,
                "cast_released": actor.hud.cast_released,
                "target_highlight": actor.hud.target_highlight,
                "combo_tracking": {"hits": actor.hud.combo_tracking.hits,
                    "position": actor.hud.combo_tracking.position}});
                json!({
                    "actor": index, "side": if actor.side == Side::Party { 0 } else { 1 },
                    "character": scene.characters.get(index),
                    "control": match actor.control {
                        Control::Manual => "manual", Control::SemiAuto => "semi_auto",
                        Control::Auto => "auto", Control::Enemy => "enemy",
                    },
                    "activity": format!("{:?}", actor.activity),
                    "availability": format!("{:?}", actor.availability),
                    "guard": format!("{:?}", actor.guard),
                    "hp": actor.hp, "max_hp": actor.max_hp, "tp": actor.tp, "max_tp": actor.max_tp,
                    "position": actor.position, "heading": actor.heading,
                    "center": actor.body.center, "center_offset": actor.body.center_offset,
                    "target_center": actor.body.target_center, "model_scale": actor.body.scale,
                    // Computed from this frame, not a stored selector input. Navigation
                    // uses the previous visit's camera and body sample before composing.
                    "derived_target_projection": frame.camera.map(|camera| json!({
                        "point": resonance_battle::project_screen_point(camera, actor.body.target_center),
                        "depth": resonance_battle::project_depth(camera, actor.body.target_center),
                    })),
                    "target_hold_ticks": target_hold_counts.get(index).copied().flatten(),
                    "target": frame.targets.get(index).copied().flatten().map(|id| id.index()),
                    "hit_stop": actor.hit_stop, "overlimit": actor.overlimit, "hud": hud
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
                    "blend_weight": model.blend_weight,
                    "root_translation": model.root_translation,
                    "world": model.world, "bones": model.bones.as_ref(),
                    "tint": model.tint, "texture_layers": model.texture_layers,
                    "light": model.light,
                    "shadow": model.shadow.map(|shadow| json!({
                        "position": shadow.position, "radius": shadow.radius,
                        "color": shadow.color,
                    })),
                })
            })
            .collect();
        Some(json!({
            "request": self.request.id(), "encounter": self.request.setup.encounter,
            "arena": self.request.setup.arena, "phase": phase,
            "update": frame.update, "input_tick": self.input_tick,
            "presentation_tick": frame.hud_update,
            "gameplay_tick": ledger.elapsed_ticks, "combat_tick": ledger.combat_ticks,
            "random_state": {"entry_seed": scene.entry_seed, "state": scene.core.random_state()},
            "recognized_result": frame.recognized_result.map(result),
            "target_selector": frame.target_selector.map(|id| id.index()),
            "target_markers": frame.target_markers.iter().map(|marker| json!({
                "owner": marker.owner.index(), "target": marker.target.index(),
                "position": marker.position, "direction": marker.direction,
                "trail": marker.trail, "phase": marker.phase,
                "control_slot": marker.control_slot,
            })).collect::<Vec<_>>(),
            "hud_holds": {"notices": frame.hud_holds.notices,
                "combo_tracking": frame.hud_holds.combo_tracking,
                "intro": frame.hud_holds.intro},
            "command": scene.lifecycle.command_frame().map(|command| json!({
                "actor": command.actor.index(), "selected": command.selected,
                "animation": command.animation, "enabled": command.enabled,
                "controller": command.controller,
                "cursor": {"current": command.cursor.current,
                    "previous": command.cursor.previous, "trail_alpha": command.cursor.trail_alpha},
            })),
            "actors": actors,
            "models": models,
            "weapons": frame.weapons.iter().map(|weapon| json!({
                "actor": weapon.owner.index(), "slot": weapon.slot,
                "resource": weapon.resource, "visible": weapon.visible,
                "clip": weapon.clip, "frame": weapon.frame,
                "world": weapon.world, "bones": weapon.bones.as_ref(),
                "tint": weapon.tint, "links": weapon.links,
            })).collect::<Vec<_>>(),
            "trails": frame.trails.iter().map(|trail| json!({
                "actor": trail.actor.index(), "slot": trail.slot,
                "resource": trail.resource,
                "rows": trail.rows.iter().map(|row| row.map(|vertex| json!({
                    "position": vertex.position, "alpha": vertex.alpha,
                }))).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "camera": frame.camera.map(|camera| json!({
                "eye": camera.eye, "target": camera.focus,
                "pitch": camera.pitch, "yaw": camera.yaw, "radius": camera.radius,
            })),
            "actions": frame.actions.iter().map(|(id, actor, age)| json!({
                "handle": format!("{id:?}"), "actor": actor.index(), "age": age,
            })).collect::<Vec<_>>(),
            "scenes": frame.scenes.iter().map(|scene| json!({
                "slot": scene.slot, "actor": scene.actor.index(), "spell": scene.spell,
                "remaining": scene.remaining,
            })).collect::<Vec<_>>(),
            "particles": frame.particles.iter().map(|particle| json!({
                "handle": format!("{:?}", particle.id), "actor": particle.owner.index(),
                "resource": particle.resource, "member": particle.member, "age": particle.age,
                "origin": particle.origin, "offset": particle.state.offset,
            })).collect::<Vec<_>>(),
            "projectile_count": frame.projectiles.len(),
            "cues": frame.cues.iter().map(|cue| format!("{cue:?}")).collect::<Vec<_>>(),
            "initial_music": scene.music, "requested_music": scene.requested_music,
        }))
    }
}
