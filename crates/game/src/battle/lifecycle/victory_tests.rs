use super::victory_selection::*;
use crate::battle::party::Character::{self, *};
use crate::battle::results::PreparedGroup;
use resonance_battle::{Control, Sound};
use resonance_content::battle_victory::Condition;

fn context<'a>(characters: &[Character], groups: &'a [PreparedGroup]) -> Context<'a> {
    Context {
        leader: characters[0],
        party: characters
            .iter()
            .map(|&character| Participant {
                character,
                available: true,
                dead: false,
                hp_percent: 100,
                close_to_lloyd: false,
                distant_from_lloyd: false,
                participation: 0,
                poisoned: false,
                control: Control::Auto,
                kills: 0,
            })
            .collect(),
        colette: ColetteState::Normal,
        presea_recovered: true,
        regal_recovered: true,
        party_was_hit: false,
        enemy_was_scanned: false,
        level_difference: 0,
        enemy_count: 1,
        seen_groups: 0,
        prepared_groups: groups,
    }
}

fn group(id: u8, participants: &[Character], condition: Condition) -> PreparedGroup {
    PreparedGroup {
        id,
        leader: participants[0] as u8,
        participants: participants.iter().map(|&id| id as u8).collect(),
        required_leader: None,
        condition,
        voice: Sound::Cue(0),
    }
}

#[test]
fn solo_celebrations_never_require_absent_companions() {
    let groups = [group(50, &[Lloyd, Sheena], Condition::Always)];
    for character in [
        Lloyd, Colette, Genis, Raine, Sheena, Zelos, Presea, Regal, Kratos,
    ] {
        for sample in [0, 1] {
            let selection = select(&context(&[character], &groups), sample);
            assert_eq!(selection.group, 0);
            assert!(selection.pose < 5);
        }
    }
}

#[test]
fn colettes_normal_poses_keep_the_chakram_grip_even_when_injured() {
    let mut context = context(&[Colette], &[]);
    for hp in [1, 25, 100] {
        context.party[0].hp_percent = hp;
        for sample in [0, 1] {
            assert!(matches!(select(&context, sample).pose, 2 | 3));
        }
    }
    context.colette = ColetteState::Alternate;
    context.party[0].hp_percent = 1;
    assert_eq!(select(&context, 0).pose, 1);
}

#[test]
fn unavailable_and_silent_characters_do_not_join_dialogue() {
    let groups = [group(15, &[Lloyd, Colette, Genis], Condition::Always)];
    let mut context = context(&[Lloyd, Colette, Genis], &groups);
    assert_eq!(select(&context, 0).group, 15);
    for state in [ColetteState::Silent, ColetteState::Sealed] {
        context.colette = state;
        assert_eq!(select(&context, 0).group, 0);
    }
    context.colette = ColetteState::Normal;
    context.party[2].available = false;
    assert_eq!(select(&context, 0).group, 0);
}

#[test]
fn fallen_companion_dialogue_needs_a_survivor_and_the_fallen_member() {
    for (condition, survivor, fallen) in [
        (Condition::RaineFallen, Sheena, vec![Raine]),
        (Condition::SheenaFallen, Raine, vec![Sheena]),
        (Condition::RaineAndSheenaFallen, Kratos, vec![Raine, Sheena]),
        (Condition::ColetteFallen, Lloyd, vec![Colette]),
        (Condition::KratosFallen, Raine, vec![Kratos]),
        (
            Condition::GenisFallenBeforePreseaRecovery,
            Presea,
            vec![Genis],
        ),
    ] {
        let characters: Vec<_> = [survivor].into_iter().chain(fallen).collect();
        let groups = [group(50, &characters, condition)];
        let mut context = context(&characters, &groups);
        context.presea_recovered = false;
        assert_eq!(select(&context, 0).group, 0);
        for member in &mut context.party[1..] {
            member.available = false;
            member.dead = true;
            member.hp_percent = 0;
            member.close_to_lloyd = true;
        }
        assert_eq!(select(&context, 0).group, 50);
        context.party[0].available = false;
        assert_eq!(select(&context, 0).group, 0);
    }
}

#[test]
fn selection_uses_prepared_dialogue_and_prefers_unheard_choices() {
    let mut groups = [
        group(50, &[Kratos, Colette, Genis], Condition::Flawless),
        group(51, &[Kratos, Colette, Genis], Condition::Always),
    ];
    groups[0].required_leader = Some(Kratos as u8);
    let mut context = context(&[Kratos, Colette, Genis], &groups);
    assert_eq!(select(&context, 0).group, 50);
    assert_eq!(select(&context, 1).group, 51);
    assert_eq!(select(&context, 2).group, 0);
    context.seen_groups = 1 << 50;
    assert_eq!(select(&context, 0).group, 51);
    context.seen_groups |= 1 << 51;
    assert_eq!(select(&context, 0).group, 50);
    context.leader = Genis;
    assert_eq!(select(&context, 0).group, 51);
    context.leader = Kratos;
    context.party_was_hit = true;
    assert_eq!(select(&context, 0).group, 51);
    context.prepared_groups = &[];
    assert_eq!(select(&context, 0).group, 0);
}
