use super::*;
use std::sync::Arc;

fn sound(index: u16) -> SoundBinding {
    SoundBinding { resource: 1, index }
}
fn battle() -> Battle {
    let mut prepared = crate::tests::prepared(
        "pub task run() {}",
        vec![
            crate::tests::actor(Side::Party),
            crate::tests::actor(Side::Enemy),
        ],
        1,
    );
    let voices = ContactVoices {
        hurt: [Some(sound(3)), Some(sound(4))],
        defeat: Some(sound(6)),
        alternate_defeat: Some(sound(117)),
        has_alternate_defeat: true,
        critical: Some(sound(8)),
        guard: Some(sound(9)),
        fifth_hit: Some(sound(40)),
        interrupted_cast: Some(sound(49)),
        affinity: [Some(sound(46)), Some(sound(47))],
        ..Default::default()
    };
    Arc::get_mut(&mut prepared).unwrap().contact_audio = Some(ContactAudio {
        actors: vec![
            ContactActorAudio {
                neutral: sound(46),
                voiced: true,
                voices,
            },
            ContactActorAudio {
                neutral: sound(54),
                voiced: false,
                voices: Default::default(),
            },
        ],
        elements: [sound(55); 8],
        guard: sound(65),
        guard_break: sound(46),
        overlimit: sound(66),
    });
    Battle::new(prepared)
}
fn hit() -> HitResult {
    HitResult {
        amount: 1,
        hp_change: -1,
        critical: false,
        affinity: Affinity::Normal,
        guard: GuardResult::None,
        auto_guard: false,
        armored: false,
        protection: HitProtection::None,
    }
}
fn rule() -> HitRule {
    HitRule {
        kind: crate::DamageKind::Slash,
        arte: false,
        power: crate::Power::Normal,
        element: crate::HitElement::Inherited,
        prevents_defeat: false,
        guard: Default::default(),
        reaction: Default::default(),
        impact: None,
    }
}
fn contact(battle: &mut Battle, owner: u8, target: u8, result: HitResult) -> Vec<Cue> {
    let mut cues = Vec::new();
    battle.contact_audio(
        ActorId(owner),
        ActorId(target),
        false,
        rule(),
        None,
        result,
        &mut cues,
    );
    cues
}
#[test]
fn unvoiced_hurt_still_draws_but_first_low_hp_contact_latches_without_drawing() {
    let mut battle = battle();
    // The shared actor fixture starts at 50/100 HP. Start above the native
    // <=50% branch so this first contact exercises the ordinary hurt roll.
    battle.actors[1].hp = 51;
    let mut expected = crate::state::Random(battle.random_state());
    expected.next();
    contact(&mut battle, 0, 1, hit());
    assert_eq!(battle.random_state(), expected.0);
    assert!(!battle.contact_audio_actors[1].low_hp);
    battle.actors[1].hp = battle.actors[1].max_hp / 2;
    contact(&mut battle, 0, 1, hit());
    assert_eq!(battle.random_state(), expected.0);
    assert!(battle.contact_audio_actors[1].low_hp);
    contact(&mut battle, 0, 1, hit());
    expected.next();
    assert_eq!(battle.random_state(), expected.0);
}
#[test]
fn weak_hits_queue_commentary_without_hurt_roll_and_share_the_battle_cooldown() {
    let mut battle = battle();
    let random = battle.random_state();
    let result = HitResult {
        affinity: Affinity::Weak,
        ..hit()
    };
    contact(&mut battle, 0, 1, result);
    assert_eq!(battle.random_state(), random);
    assert_eq!(battle.voices[0].secondary, Some((sound(47), 3)));
    assert_eq!(battle.contact_audio_affinity, 480);
    battle.voices[0].secondary = None;
    contact(&mut battle, 0, 1, result);
    assert!(battle.voices[0].secondary.is_none());
}
#[test]
fn fifth_hit_precedes_low_hp_and_guard_repetition_stops_lower_priority_voice() {
    let mut battle = battle();
    let random = battle.random_state();
    battle.actors[0].hp = battle.actors[0].max_hp / 3;
    battle.actors[0].reaction.combo_hits = 5;
    contact(&mut battle, 1, 0, hit());
    assert_eq!(battle.voices[0].pending, Some((sound(40), 2)));
    assert!(!battle.contact_audio_actors[0].low_hp);
    assert_eq!(battle.random_state(), random);
    let result = HitResult {
        guard: GuardResult::Blocked {
            first: true,
            special: false,
        },
        ..hit()
    };
    contact(&mut battle, 1, 0, result);
    assert_eq!(battle.voices[0].pending, Some((sound(9), 1)));
    let mut cues = Vec::new();
    battle.voices[0]
        .step(ActorId(0), [0.; 3], true, &mut battle.next_voice, &mut cues)
        .unwrap();
    let playing = battle.voices[0].playing.unwrap();
    let cues = contact(&mut battle, 1, 0, result);
    assert!(
        matches!(cues.as_slice(), [Cue::Sound { sound: SoundBinding { index: 65, .. }, .. }, Cue::VoiceStopped { playback }] if *playback == playing)
    );
    assert_eq!(battle.contact_audio_actors[0].guard, 240);
    assert_eq!(battle.random_state(), random);
}
#[test]
fn lethal_party_alternate_draw_follows_original_mask_replacement() {
    let mut battle = battle();
    battle.actors[0].hp = 0;
    let mut expected = crate::state::Random(battle.random_state());
    let index = if expected.next() & 1 == 0 { 117 } else { 6 };
    contact(
        &mut battle,
        1,
        0,
        HitResult {
            affinity: Affinity::Weak,
            ..hit()
        },
    );
    assert_eq!(battle.voices[0].pending, Some((sound(index), 3)));
    assert_eq!(battle.random_state(), expected.0);
    assert_eq!(battle.contact_audio_affinity, 0);
}
