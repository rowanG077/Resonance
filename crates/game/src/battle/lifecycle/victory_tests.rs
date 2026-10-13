use super::victory_selection::*;
use crate::battle::party::Character::{self, *};
use crate::battle::results::PreparedGroup;
use resonance_battle::{Control, Sound};
use resonance_content::battle_victory::Condition;

fn context<'a>(characters: &[Character], groups: &'a [PreparedGroup]) -> Context<'a> {
    Context {
        leader: characters[0],
        formation: 1,
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
    let groups = [group(15, &[Lloyd, Colette, Genis], Condition::Always)];
    for character in [
        Lloyd, Colette, Genis, Raine, Sheena, Zelos, Presea, Regal, Kratos,
    ] {
        for sample in [0, 1] {
            let selection = select(&context(&[character], &groups), || sample);
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
            assert!(matches!(select(&context, || sample).pose, 2 | 3));
        }
    }
    context.colette = ColetteState::Alternate;
    context.party[0].hp_percent = 1;
    assert_eq!(select(&context, || 0).pose, 1);
}

#[test]
fn unavailable_and_silent_characters_do_not_join_dialogue() {
    let groups = [group(15, &[Lloyd, Colette, Genis], Condition::Always)];
    let mut context = context(&[Lloyd, Colette, Genis], &groups);
    assert_eq!(select(&context, || 0).group, 15);
    for state in [ColetteState::Silent, ColetteState::Sealed] {
        context.colette = state;
        assert_eq!(select(&context, || 0).group, 0);
    }
    context.colette = ColetteState::Normal;
    context.party[2].available = false;
    assert_eq!(select(&context, || 0).group, 0);
}

#[test]
fn fallen_companion_dialogue_needs_a_survivor_and_the_fallen_member() {
    for (id, condition, survivor, fallen) in [
        (2, Condition::RaineFallen, Sheena, vec![Raine]),
        (3, Condition::SheenaFallen, Raine, vec![Sheena]),
        (
            4,
            Condition::RaineAndSheenaFallen,
            Kratos,
            vec![Raine, Sheena],
        ),
        (26, Condition::ColetteFallen, Lloyd, vec![Colette]),
        (37, Condition::KratosFallen, Raine, vec![Kratos]),
        (
            40,
            Condition::GenisFallenBeforePreseaRecovery,
            Presea,
            vec![Genis],
        ),
    ] {
        let characters: Vec<_> = [survivor].into_iter().chain(fallen).collect();
        let groups = [group(id, &characters, condition)];
        let mut context = context(&characters, &groups);
        context.presea_recovered = false;
        assert_eq!(select(&context, || 0).group, 0);
        for member in &mut context.party[1..] {
            member.available = false;
            member.dead = true;
            member.hp_percent = 0;
            member.close_to_lloyd = true;
        }
        let mut draws = [0, if id == 26 { 4 } else { 1 }].into_iter();
        assert_eq!(select(&context, || draws.next().unwrap()).group, id);
        context.party[0].available = false;
        assert_eq!(select(&context, || 0).group, 0);
    }
}

#[test]
fn group_selection_keeps_ordered_odds_and_only_draws_for_eligible_choices() {
    let groups = [group(15, &[Lloyd, Colette, Genis], Condition::Always)];
    let isolated = context(&[Lloyd, Colette, Genis], &groups);
    let mut selected = 0;
    for gate in 0..2 {
        for branch in 0..8 {
            let mut draws = [gate, branch, 1].into_iter();
            selected += usize::from(select(&isolated, || draws.next().unwrap()).group == 15);
        }
    }
    assert_eq!(selected, 1, "group 15 needs both the half and eighth gates");

    // Input order and completion history do not replace exchange precedence.
    let groups = [
        group(39, &[Kratos, Colette, Genis], Condition::Always),
        group(38, &[Kratos, Colette, Genis], Condition::Flawless),
    ];
    let mut context = context(&[Kratos, Colette, Genis], &groups);
    context.seen_groups = 1 << 38;
    for (draws, expected) in [(vec![0, 1], 38), (vec![0, 0, 0], 39)] {
        let mut draws = draws.into_iter();
        assert_eq!(select(&context, || draws.next().unwrap()).group, expected);
        assert_eq!(draws.next(), None);
    }
    context.party_was_hit = true;
    let mut draws = [0, 0].into_iter();
    assert_eq!(select(&context, || draws.next().unwrap()).group, 39);
    assert_eq!(draws.next(), None, "ineligible group 38 consumes no draw");
}

#[test]
fn encounter_exchange_uses_prepared_roster_priority_without_random_draws() {
    let groups = [
        group(46, &[Lloyd], Condition::Always),
        group(47, &[Lloyd, Kratos], Condition::Always),
    ];
    let mut context = context(&[Lloyd, Kratos], &groups);
    context.formation = 31;
    context.party[1].dead = true;
    context.party[1].available = false;
    assert_eq!(
        select(&context, || panic!("forced group drew randomness")).group,
        47
    );
    context.prepared_groups = &groups[..1];
    assert_eq!(
        select(&context, || panic!("forced group drew randomness")).group,
        46
    );
    context.prepared_groups = &groups;
    context.party.pop();
    assert_eq!(
        select(&context, || panic!("forced group drew randomness")).group,
        46
    );
}
