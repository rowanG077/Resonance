//! Optional scene resources. None of these participate in battle admission or gameplay.
use resonance_battle::{ActorId, Cue, EffectAppearance, MotionBinding, RescueKind, Sound};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Feedback {
    pub actors: Vec<ActorFeedback>,
    pub casting: BTreeMap<resonance_battle::ActionKey, CastingFeedback>,
    pub unison_ready: Option<Sound>,
    pub overlimit_sound: Option<Sound>,
    pub hammer: Option<EffectAppearance>,
    pub takeoff: Option<EffectAppearance>,
    pub skill_ready: Option<EffectAppearance>,
    pub counter: Option<EffectAppearance>,
    pub entry_voice: Option<(ActorId, Sound)>,
    pub items: Option<super::items::Feedback>,
    pub contact_art: Option<super::contact_feedback::ContactArt>,
    pub contact_audio: Option<super::contact_audio::ContactAudio>,
    pub admission_flashes: BTreeMap<resonance_battle::ActionKey, [u8; 3]>,
    pub recovery_tint: Option<[u8; 3]>,
    pub self_cure_notice: Option<String>,
    pub rescue_names: [String; 5],
    pub rescues: Vec<RescueFeedback>,
    pub breakfalls: Vec<Option<BreakfallFeedback>>,
    pub death: Option<super::death::Feedback>,
    pub landing: Option<EffectAppearance>,
}

pub struct CastingFeedback {
    pub chant_voice: Option<Sound>,
    pub release_voice: Option<Sound>,
    pub start_sound: Option<Sound>,
    pub release_sound: Option<Sound>,
    pub chant_effect: EffectAppearance,
    pub charged_effect: EffectAppearance,
    pub stored_effect: EffectAppearance,
    pub release_effect: EffectAppearance,
    pub tint: resonance_content::battle_effect::EffectTint,
}

#[derive(Default)]
pub struct ActorFeedback {
    pub overlimit_voice: Option<Sound>,
    pub technique_command: Option<Sound>,
    pub taunt: Option<Sound>,
    pub charge: Option<Sound>,
    pub charge_failed: Option<Sound>,
    pub backstep: Option<Sound>,
    pub knockdown: Option<Sound>,
}

pub struct BreakfallFeedback {
    pub effect: EffectAppearance,
    pub voice: Option<Sound>,
    pub sound: Option<Sound>,
}

pub struct RescueFeedback {
    pub motion: Option<MotionBinding>,
    pub expression: Option<[u8; 4]>,
    pub appearance: EffectAppearance,
    pub sound: Option<Sound>,
}

impl Feedback {
    pub fn notice(&self, cue: &Cue) -> Option<(ActorId, &str)> {
        let (actor, text) = match *cue {
            Cue::SelfCured { actor } => (actor, self.self_cure_notice.as_deref()?),
            Cue::Rescued { actor, kind } => (
                actor,
                self.rescue_names[match kind {
                    RescueKind::AngelTear => 0,
                    RescueKind::Revive => 1,
                    RescueKind::Resurrect => 2,
                    RescueKind::Ring => 3,
                    RescueKind::Doll(_) => 4,
                }]
                .as_str(),
            ),
            _ => return None,
        };
        (!text.is_empty()).then_some((actor, text))
    }
}
