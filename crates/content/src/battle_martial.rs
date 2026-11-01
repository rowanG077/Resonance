//! Party melee techniques with resolved gameplay timelines.
use crate::{battle_action::ActionPose, battle_normal::Event};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod document;

/// A mod-authored arte owns its timeline and optional motion/effect bank.
/// Motion slots are local to the character; conflicting declarations are errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredAction {
    #[serde(deserialize_with = "read_authored_attack")]
    pub attack: Attack,
    #[serde(default)]
    pub motions: BTreeMap<u16, String>,
    #[serde(default)]
    pub effects: Option<String>,
    pub unison: crate::battle_unison::Placement,
}

fn read_authored_attack<'de, D>(deserializer: D) -> Result<Attack, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let attack = Attack::deserialize(deserializer)?;
    // Native recipes use specialized preparers; the generic timeline runner
    // must not silently discard their additional behavior.
    for (field, present) in [
        ("flight", attack.flight.is_some()),
        ("hammer", attack.hammer.is_some()),
        ("steal", attack.steal.is_some()),
        ("fierce_fang", attack.fierce_fang.is_some()),
        ("hurricane_hit", attack.hurricane_hit.is_some()),
        ("callback_hits", !attack.callback_hits.is_empty()),
    ] {
        if present {
            return Err(serde::de::Error::custom(format!(
                "authored actions do not support native recipe field `{field}`"
            )));
        }
    }
    Ok(attack)
}

/// Every common action is cooked independently of the characters using it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalogue {
    pub members: BTreeMap<u16, Member>,
    pub bindings: BTreeMap<u16, BTreeMap<u8, Binding>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub variants: BTreeMap<u8, crate::battle_scene::SceneAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Binding {
    pub member: u16,
    pub variant: u8,
}

impl Binding {
    pub fn enemy(member: u16) -> Self {
        Self {
            member,
            variant: u8::from(matches!(member, 1 | 2 | 4)),
        }
    }
}

impl Catalogue {
    pub fn attack(&self, catalogue: u16, character: u8) -> Result<&Attack> {
        let binding = self
            .bindings
            .get(&catalogue)
            .and_then(|characters| characters.get(&character))
            .with_context(|| {
                format!("missing martial binding {catalogue} for character {character}")
            })?;
        self.variant(*binding)
    }

    pub fn variant(&self, binding: Binding) -> Result<&Attack> {
        let member = self
            .members
            .get(&binding.member)
            .with_context(|| format!("missing common action {}", binding.member))?;
        if let Some(reason) = &member.unsupported_reason {
            bail!("common action {} is not prepared: {reason}", binding.member);
        }
        member
            .variants
            .get(&binding.variant)
            .with_context(|| {
                format!(
                    "missing common action {} variant {}",
                    binding.member, binding.variant
                )
            })?
            .prepared()
    }
}

/// Resolve a gameplay event after earlier melee completions and animation blends.
/// Pose blend ages are authored times, before any holds have been applied.
pub fn gameplay_tick(
    raw_at: u16,
    preceding_contacts: u16,
    pose_blends: &[(u16, u8)],
) -> Result<u16> {
    let mut at = u32::from(raw_at) + u32::from(preceding_contacts);
    let mut pose_delay = 0;
    for &(start, blend) in pose_blends {
        if start == 0 {
            at += u32::from(blend);
        } else if u32::from(start) + pose_delay < at {
            at += u32::from(blend.saturating_sub(1));
        }
        pose_delay += u32::from(blend);
    }
    at.try_into()
        .context("martial gameplay timeline exceeds limit")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "E: Deserialize<'de>"), deny_unknown_fields)]
pub struct Attack<E = Event> {
    pub duration: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_at: Option<u16>,
    pub recovery: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<u16>,
    pub events: Vec<(u16, E)>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "read_callback_hits"
    )]
    pub callback_hits: BTreeMap<u8, crate::battle_action::ActionHit>,
    /// Model-dependent pose completion is resolved once during action preparation.
    /// These actions retain authored event, duration, and chaining timestamps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose_sequence: Option<Vec<PoseStep>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flight: Option<Flight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hammer: Option<Hammer<E>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steal: Option<Steal<E>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fierce_fang: Option<FierceFang>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hurricane_hit: Option<crate::battle_action::ActionHit>,
}

// Internally tagged SceneAction buffers JSON keys as strings before reading Attack.
fn read_callback_hits<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<u8, crate::battle_action::ActionHit>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    BTreeMap::<String, crate::battle_action::ActionHit>::deserialize(deserializer)?
        .into_iter()
        .map(|(slot, hit)| {
            slot.parse()
                .map(|slot| (slot, hit))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseStep {
    pub start: PoseStart,
    pub pose: ActionPose,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoseStart {
    At(u16),
    AfterPrevious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectileFeedback {
    pub effect: u16,
    pub sound: u16,
    pub on_guard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FierceFang {
    /// Ordinary shockwave and Earth projectile damage.
    pub hits: [crate::battle_action::ActionHit; 2],
}

/// An aerial strike changes gravity at launch and finishes on the floor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flight {
    pub gravity_at: u16,
    pub gravity: f32,
    pub finish_pose: ActionPose,
}

/// Hammer contacts share one template; each launch pattern owns its motion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hammer<E = Event> {
    pub pattern: HammerPattern<E>,
    /// Plain, Ice, and Toss Hammer contacts, in selection order.
    pub hits: [crate::battle_action::ActionHit; 3],
    pub impact_effect: u16,
    pub impact_sound: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HammerPattern<E = Event> {
    Single {
        alternate_events: Vec<(u16, E)>,
    },
    Volley {
        offset: [f32; 3],
        forward: f32,
        jitter: f32,
    },
    Ring {
        offset: [f32; 3],
        step: f32,
        radians: f32,
        speed: f32,
        speed_jitter: f32,
        lift: f32,
        lift_jitter: f32,
        opening_hit: crate::battle_action::ActionHit,
    },
}

/// Item Thief changes only its remaining track after a successful transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Steal<E = Event> {
    pub attempt_at: u16,
    pub bonus_at: u16,
    pub success_duration: u16,
    pub success_events: Vec<(u16, E)>,
    pub bonus_hit: crate::battle_action::ActionHit,
    pub bonus_offset: [f32; 3],
    pub bonus_forward_speed: f32,
    pub bonus_velocity_jitter: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::gameplay_tick;

    #[test]
    fn gameplay_waits_for_crossed_blends_but_not_simultaneous_admissions() {
        let poses = [(0, 4), (14, 4), (28, 2), (48, 4), (60, 3)];
        assert_eq!(gameplay_tick(0, 0, &poses).unwrap(), 4);
        assert_eq!(gameplay_tick(48, 3, &poses).unwrap(), 62);
        assert_eq!(gameplay_tick(25, 0, &[(0, 4), (25, 4)]).unwrap(), 29);
        assert_eq!(gameplay_tick(29, 3, &[(0, 4)]).unwrap(), 36);
        // Commands run before a newly selected pose starts its blend.
        let poses = [(0, 2), (11, 4), (27, 4), (42, 2)];
        assert_eq!(gameplay_tick(28, 0, &poses).unwrap(), 33);
        assert_eq!(gameplay_tick(29, 0, &poses).unwrap(), 37);
    }
}

#[cfg(test)]
mod authored_tests {
    use super::*;

    fn action_document() -> serde_json::Value {
        serde_json::json!({
            "attack": {
                "duration": 20,
                "chain_at": 16,
                "recovery": 4,
                "effect": 0,
                "events": [[6, {"forward": 2.0}]],
                "pose_sequence": [{"start": {"at": 0}, "pose": {"clip": "slot-110"}}]
            },
            "motions": {"110": "battle/artes/example/motion.glb"},
            "effects": "battle/artes/example/effects.json",
            "unison": {"native_id": 1000, "altitude": 0, "distance": 80, "duration": 24}
        })
    }

    #[test]
    fn authored_actions_preserve_generic_timeline_and_resources() -> Result<()> {
        let document = action_document();
        let action: AuthoredAction = serde_json::from_value(document.clone())?;
        assert_eq!(serde_json::to_value(action)?, document);
        Ok(())
    }

    #[test]
    fn authored_actions_reject_native_recipes_without_changing_native_schema() -> Result<()> {
        let hit = serde_json::json!({"kind": "slash", "power": {"percent": 100}});
        let recipes = [
            (
                "flight",
                serde_json::json!({
                    "gravity_at": 2, "gravity": 0.5, "finish_pose": {"clip": "slot-110"}
                }),
            ),
            (
                "hammer",
                serde_json::json!({
                    "pattern": {"kind": "single", "alternate_events": []},
                    "hits": [hit.clone(), hit.clone(), hit.clone()],
                    "impact_effect": 0, "impact_sound": 60
                }),
            ),
            (
                "steal",
                serde_json::json!({
                    "attempt_at": 2, "bonus_at": 3, "success_duration": 10,
                    "success_events": [], "bonus_hit": hit.clone(), "bonus_offset": [0.0, 0.0, 0.0],
                    "bonus_forward_speed": 1.0, "bonus_velocity_jitter": [0.0, 0.0, 0.0]
                }),
            ),
            (
                "fierce_fang",
                serde_json::json!({"hits": [hit.clone(), hit.clone()]}),
            ),
            ("hurricane_hit", hit.clone()),
            ("callback_hits", serde_json::json!({"0": hit})),
        ];
        for (field, value) in recipes {
            let mut document = action_document();
            document["attack"][field] = value;
            let _: Attack = serde_json::from_value(document["attack"].clone())?;
            let error = serde_json::from_value::<AuthoredAction>(document).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("native recipe field `{field}`")),
                "unexpected rejection for {field}: {error}"
            );
        }
        Ok(())
    }
}
