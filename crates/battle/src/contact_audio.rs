//! Ordinary contact sound and voice tail (3B228 / 3AD24). Voice selection is
//! simulation work: even an unvoiced actor consumes the ordinary hurt roll.
use crate::{
    Activity, ActorId, Affinity, Battle, Cue, Element, GuardResult, HitProtection, HitResult,
    HitRule, PreparedBattle, Side, SoundBinding,
};
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Default)]
pub struct ContactVoices {
    pub hurt: [Option<SoundBinding>; 2],
    pub low_hp: Option<SoundBinding>,
    pub defeat: Option<SoundBinding>,
    pub alternate_defeat: Option<SoundBinding>,
    /// Retained independently of an absent/disabled actor voice binding.
    pub has_alternate_defeat: bool,
    pub critical: Option<SoundBinding>,
    pub guard: Option<SoundBinding>,
    pub arte_guard_break: Option<SoundBinding>,
    pub fifth_hit: Option<SoundBinding>,
    pub interrupted_cast: Option<SoundBinding>,
    pub stunned: Option<SoundBinding>,
    pub stunned_override: bool,
    /// Resistance/absorption, then weakness (relative lines 46 and 47).
    pub affinity: [Option<SoundBinding>; 2],
    pub kill: Option<SoundBinding>,
}

#[derive(Debug, Clone)]
pub struct ContactActorAudio {
    pub neutral: SoundBinding,
    pub voiced: bool,
    pub voices: ContactVoices,
}

#[derive(Debug, Clone)]
pub struct ContactAudio {
    pub actors: Vec<ContactActorAudio>,
    pub elements: [SoundBinding; 8],
    pub guard: SoundBinding,
    pub guard_break: SoundBinding,
    pub overlimit: SoundBinding,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ActorState {
    guard: u8,
    kill: u16,
    low_hp: bool,
}
impl ActorState {
    pub(crate) fn step(&mut self) {
        self.guard = self.guard.saturating_sub(1);
        self.kill = self.kill.saturating_sub(1);
    }
}

impl PreparedBattle {
    pub fn with_contact_audio(mut self, audio: ContactAudio) -> Result<Self> {
        ensure!(
            audio.actors.len() == self.actors.len(),
            "contact audio/actor count differs"
        );
        self.contact_audio = Some(audio);
        Ok(self)
    }
}

impl Battle {
    // These are the resolved contact operands, shared with damage and feedback.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn contact_audio(
        &mut self,
        owner: ActorId,
        target: ActorId,
        was_casting: bool,
        rule: HitRule,
        element: Option<Element>,
        result: HitResult,
        cues: &mut Vec<Cue>,
    ) {
        let Some(audio) = self.prepared.contact_audio.as_ref() else {
            return;
        };
        let source = &audio.actors[owner.index()];
        let victim = &audio.actors[target.index()];
        let actor = &self.actors[target.index()];
        let lethal = actor.hp == 0;
        // The lethal wrapper replaces the damage mask; affinity no longer
        // affects commentary or voice suppression for that contact.
        let affinity = if lethal {
            Affinity::Normal
        } else {
            result.affinity
        };
        let sound = if actor.overlimit_active {
            audio.overlimit
        } else if result.guard == GuardResult::Broken {
            audio.guard_break
        } else if matches!(result.guard, GuardResult::Blocked { .. }) {
            audio.guard
        } else {
            element.map_or(source.neutral, |element| audio.elements[element as usize])
        };
        cues.push(Cue::Sound {
            actor: target,
            sound,
            position: actor.body.audio_position,
            priority: 1,
        });
        let owner_party = self.actors[owner.index()].side == Side::Party;
        let target_party = actor.side == Side::Party;
        if lethal && self.contact_audio_actors[owner.index()].kill == 0 {
            if owner_party
                && self
                    .actors
                    .iter()
                    .filter(|a| a.side == actor.side && a.available())
                    .count()
                    >= 2
            {
                if let Some(sound) = source.voices.kill {
                    self.voices[owner.index()].enqueue(sound, 3);
                }
                self.contact_audio_actors[owner.index()].kill = 360;
            }
        } else if self.contact_audio_affinity == 0 {
            let variant = match affinity {
                Affinity::Absorb | Affinity::Immune => Some(0),
                Affinity::Weak => Some(1),
                _ => None,
            };
            if let Some(variant) = variant {
                if let Some(sound) = source.voices.affinity[variant] {
                    self.voices[owner.index()].enqueue(sound, 3);
                }
                self.contact_audio_affinity = 480;
            }
        }
        let request = |voices: &mut [crate::voice::Voice], sound, priority| {
            if let Some(sound) = sound {
                voices[target.index()].request(sound, priority);
            }
        };
        if lethal {
            let alternate = victim.voices.has_alternate_defeat
                && (!target_party || self.random.next() & 1 == 0);
            request(
                &mut self.voices,
                if alternate {
                    victim.voices.alternate_defeat
                } else {
                    victim.voices.defeat
                },
                3,
            );
        } else if result.guard == GuardResult::Broken {
            request(
                &mut self.voices,
                if rule.arte {
                    victim.voices.arte_guard_break
                } else {
                    victim.voices.critical
                },
                2,
            );
        } else if let GuardResult::Blocked { first, .. } = result.guard {
            if first {
                let state = &mut self.contact_audio_actors[target.index()];
                if state.guard == 0 {
                    request(&mut self.voices, victim.voices.guard, 1);
                } else if victim.voiced
                    && self.voices[target.index()].priority <= 2
                    && let Some(playback) = self.voices[target.index()].playing.take()
                {
                    cues.push(Cue::VoiceStopped { playback });
                }
                state.guard = 240;
            }
        } else if result.armored
            || result.protection != HitProtection::None
            || actor.overlimit_active
            || matches!(affinity, Affinity::Absorb | Affinity::Immune)
        {
            // Original result bits 2/4 suppress ordinary hurt voices.
        } else if affinity == Affinity::Weak && owner_party {
            // 15968 starts clear; native 3AD24 tests an already-set bit before
            // ORing it again. Its immediate line is therefore unreachable in
            // an ordinary battle. The secondary request above still occurs.
        } else if result.critical {
            request(&mut self.voices, victim.voices.critical, 2);
        } else if actor.reaction.combo_hits == 5 {
            request(&mut self.voices, victim.voices.fifth_hit, 2);
        } else if actor.hp.wrapping_mul(100) / actor.max_hp <= 50
            && !self.contact_audio_actors[target.index()].low_hp
        {
            request(&mut self.voices, victim.voices.low_hp, 3);
            self.contact_audio_actors[target.index()].low_hp = true;
        } else if was_casting {
            request(&mut self.voices, victim.voices.interrupted_cast, 2);
        } else if actor.activity == Activity::Stunned && victim.voices.stunned_override {
            request(&mut self.voices, victim.voices.stunned, 2);
        } else {
            let variant = usize::from(self.random.next() & 1);
            request(&mut self.voices, victim.voices.hurt[variant], 2);
        }
    }
}

#[cfg(test)]
mod tests;
