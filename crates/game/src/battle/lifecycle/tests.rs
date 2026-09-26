use super::*;
use resonance_battle::{
    Actor, ActorAvailability, BattlePhase, BattleResult, ButtonInput, PreparedBattle, Side,
};

const SOURCE: &str = include_str!("../../../../../scripts/battle/encounter.sym");

fn actor(side: Side, hp: i32) -> Actor {
    Actor {
        side,
        control: Default::default(),
        activity: Default::default(),
        availability: ActorAvailability::Active,
        guard: Default::default(),
        hp,
        max_hp: 100,
        tp: 20,
        max_tp: 30,
        hud: Default::default(),
        overlimit: 0,
        overlimit_active: false,
        luck: 0,
        stats: Default::default(),
        elements: Default::default(),
        affinities: [resonance_battle::Affinity::Normal; 9],
        attack_power: 100,
        physical_arte_boost: false,
        recovery: Default::default(),
        petrified: false,
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        framing: Default::default(),
        body: Default::default(),
        movement: Default::default(),
        reaction: Default::default(),
        hit_stop: 0,
    }
}
fn battle(party_hp: i32, enemy_hp: i32) -> Battle {
    battle_seed(party_hp, enemy_hp, 1)
}
fn battle_seed(party_hp: i32, enemy_hp: i32, seed: u32) -> Battle {
    Battle::new(Arc::new(
        PreparedBattle::new(
            vec![actor(Side::Party, party_hp), actor(Side::Enemy, enemy_hp)],
            vec![],
            seed,
            vec![],
            vec![],
        )
        .unwrap(),
    ))
}
fn lifecycle(source: &str) -> Lifecycle {
    let sources = BTreeMap::from([
        ("battle::encounter".into(), source.into()),
        (
            "battle::victory".into(),
            include_str!("../../../../../scripts/battle/victory.sym").into(),
        ),
    ]);
    PreparedLifecycle::prepare(&mut PreparationCache::default(), &sources)
        .unwrap()
        .start()
        .unwrap()
}
#[derive(Default)]
struct Accepted {
    requests: Vec<Request>,
    stale: bool,
    fail_after_world: bool,
    previous: Option<Acknowledgement>,
}
impl Services for Accepted {
    fn after_world(&mut self, _: &BattleFrame) -> Result<()> {
        ensure!(!self.fail_after_world, "injected display failure");
        Ok(())
    }
    fn observations(&self, _: &Battle) -> Observations {
        Observations {
            music_ready: true,
            performance_ready: true,
            performance_finished: true,
            results_ready: true,
            ..Default::default()
        }
    }
    fn selection_query(&self, query: SelectionQuery, _: &Battle) -> Result<i32> {
        Ok(match query {
            SelectionQuery::Leader => 1,
            SelectionQuery::Available(1) | SelectionQuery::AllHealthy => 1,
            SelectionQuery::HpPercent(_) => 100,
            _ => 0,
        })
    }
    fn request(&mut self, request: Request, _: &mut Battle) -> Result<Acknowledgement> {
        self.requests.push(request);
        if self.stale
            && let Some(previous) = self.previous.clone()
        {
            return Ok(previous);
        }
        let ack = request.acknowledge();
        self.previous = Some(ack.clone());
        Ok(ack)
    }
}

#[test]
fn maintained_victory_runs_tp_before_completion_and_never_returns_twice() {
    let mut lifecycle = lifecycle(SOURCE);
    let mut battle = battle(100, 0);
    let mut services = Accepted::default();
    let mut outcome = None;
    for update in 0..400 {
        let frame = lifecycle
            .step(
                &mut battle,
                Input {
                    confirm: true,
                    ..Default::default()
                },
                &mut services,
            )
            .unwrap();
        if update == 0 {
            assert_eq!(frame.recognized_result, Some(BattleResult::Victory));
            assert!(frame.outcome.is_none());
        }
        if frame.outcome.is_some() {
            outcome = frame.outcome;
            break;
        }
    }
    let outcome = outcome.expect("maintained victory never returned");
    assert_eq!(outcome.result, BattleResult::Victory);
    let kinds: Vec<_> = services
        .requests
        .iter()
        .map(|request| request.kind)
        .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == RequestKind::ConstructRewards)
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == RequestKind::RecoverTp)
            .count(),
        1
    );
    let tp = kinds
        .iter()
        .position(|kind| *kind == RequestKind::RecoverTp)
        .unwrap();
    assert_eq!(
        kinds[tp - 1],
        RequestKind::UpdateResults {
            age: 150,
            confirm: true
        }
    );
    assert!(kinds[..tp].contains(&RequestKind::PerformVictory));
    let mut random = 1u32;
    // One group-selection draw precedes the 480 wipe rows.
    for _ in 0..481 {
        random = random.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12_d687);
    }
    assert_eq!(outcome.random_state, random);
    assert!(
        lifecycle
            .step(&mut battle, Input::default(), &mut services)
            .is_err()
    );
    assert!(battle.finish_result().is_err());
}

#[test]
fn menu_pause_holds_authored_result_age_but_keeps_the_same_live_owner() {
    let mut lifecycle = lifecycle(SOURCE);
    let mut battle = battle(100, 0);
    let mut services = Accepted::default();
    for _ in 0..70 {
        lifecycle
            .step(&mut battle, Input::default(), &mut services)
            .unwrap();
    }
    let requests = services.requests.len();
    let before = battle.snapshot();
    let frame = lifecycle
        .step(
            &mut battle,
            Input {
                battle: BattleInput {
                    menu_open: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &mut services,
        )
        .unwrap();
    assert_eq!(frame.update, before.update);
    assert_eq!(services.requests.len(), requests);
    assert!(frame.outcome.is_none());
    lifecycle
        .step(&mut battle, Input::default(), &mut services)
        .unwrap();
    assert!(services.requests.len() > requests);
}

#[test]
fn defeat_and_escape_do_not_construct_victory_rewards() {
    for escaped in [false, true] {
        let mut lifecycle = lifecycle(SOURCE);
        let mut battle = battle(if escaped { 100 } else { 0 }, 100);
        if escaped {
            battle.recognize_escape(false).unwrap();
        }
        let mut services = Accepted::default();
        let mut outcome = None;
        for _ in 0..300 {
            let frame = lifecycle
                .step(
                    &mut battle,
                    Input {
                        confirm: true,
                        ..Default::default()
                    },
                    &mut services,
                )
                .unwrap();
            if frame.outcome.is_some() {
                outcome = frame.outcome;
                break;
            }
        }
        assert_eq!(
            outcome.unwrap().result,
            if escaped {
                BattleResult::Escaped
            } else {
                BattleResult::Defeat
            }
        );
        assert!(!services.requests.iter().any(|request| matches!(
            request.kind,
            RequestKind::ConstructRewards | RequestKind::RecoverTp
        )));
        assert_eq!(
            services
                .requests
                .iter()
                .filter(|request| request.kind == RequestKind::RecordEscape)
                .count(),
            usize::from(escaped)
        );
    }
}

#[test]
fn mismatched_acknowledgement_faults_before_a_result_can_be_committed() {
    let mut lifecycle = lifecycle(SOURCE);
    let mut battle = battle(100, 0);
    let mut services = Accepted {
        stale: true,
        ..Default::default()
    };
    let error = lifecycle
        .step(&mut battle, Input::default(), &mut services)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("mismatched encounter acknowledgement")
    );
    assert_eq!(battle.phase(), BattlePhase::Finished);
    assert!(battle.finish_result().is_err());
    assert!(
        lifecycle
            .step(&mut battle, Input::default(), &mut services)
            .is_err()
    );
}

struct SelectionScenario {
    accepted: Accepted,
    leader: u8,
    present: [bool; 4],
    rank: [i32; 4],
    story: i32,
}
impl Services for SelectionScenario {
    fn observations(&self, battle: &Battle) -> Observations {
        self.accepted.observations(battle)
    }
    fn selection_query(&self, query: SelectionQuery, _: &Battle) -> Result<i32> {
        Ok(match query {
            SelectionQuery::Leader => i32::from(self.leader),
            SelectionQuery::Available(id) => i32::from(self.present[usize::from(id)]),
            SelectionQuery::AffinityRank(id) => self.rank[usize::from(id)],
            SelectionQuery::HpPercent(_) => 100,
            SelectionQuery::Story => self.story,
            SelectionQuery::AllHealthy => 1,
            _ => 0,
        })
    }
    fn request(&mut self, request: Request, battle: &mut Battle) -> Result<Acknowledgement> {
        self.accepted.request(request, battle)
    }
}

#[test]
fn victory_group_selection_preserves_source_short_circuit_random_order() {
    for (seed, leader, present, rank, story, expected, draws) in [
        // 80B20 group5: first even, second odd, highest-affection Genis leads.
        (
            4,
            3,
            [false, true, false, true],
            [8, 8, 1, 0],
            0,
            RequestKind::SelectVictory { group: 5, pose: 0 },
            2,
        ),
        // 1630 Colette story0 consumes the ordinary selector draw before HP tests.
        (
            1,
            2,
            [false, true, true, false],
            [8, 8, 0, 1],
            0,
            RequestKind::SelectVictory { group: 0, pose: 3 },
            2,
        ),
        // Group15 tests RNG before its failing story range; all-healthy pose0.
        (
            4,
            1,
            [false, true, true, true],
            [8, 8, 0, 1],
            500,
            RequestKind::SelectVictory { group: 0, pose: 0 },
            2,
        ),
    ] {
        let mut lifecycle = lifecycle(SOURCE);
        let mut battle = battle_seed(100, 0, seed);
        let mut services = SelectionScenario {
            accepted: Accepted::default(),
            leader,
            present,
            rank,
            story,
        };
        lifecycle
            .step(&mut battle, Input::default(), &mut services)
            .unwrap();
        assert_eq!(services.accepted.requests[0].kind, expected);
        let mut expected_state = seed;
        for _ in 0..draws {
            expected_state = expected_state
                .wrapping_mul(0x41c6_4e6d)
                .wrapping_add(0x12_d687);
        }
        assert_eq!(battle.random_state(), expected_state);
    }
}

#[test]
fn prepared_source_admission_opens_command_before_the_authored_vm() {
    let sources = BTreeMap::from([
        ("battle::encounter".into(), SOURCE.into()),
        (
            "battle::victory".into(),
            include_str!("../../../../../scripts/battle/victory.sym").into(),
        ),
    ]);
    let prepared = Arc::new(
        PreparedBattle::new(
            vec![actor(Side::Party, 100), actor(Side::Enemy, 100)],
            vec![],
            1,
            vec![],
            vec![],
        )
        .unwrap(),
    );
    let actor_id = prepared.actor_ids().next().unwrap();
    let mut battle = Battle::new(prepared);
    let lifecycle = PreparedLifecycle::prepare(&mut PreparationCache::default(), &sources)
        .unwrap()
        .with_command_setup(crate::battle::command::Setup {
            actors: vec![actor_id],
            enabled: 0xdd,
        })
        .start()
        .unwrap();
    let mut lifecycle = lifecycle;
    let mut services = Accepted::default();
    let frame = lifecycle
        .step(
            &mut battle,
            Input {
                command: crate::battle::command::Input {
                    open: ButtonInput {
                        held: true,
                        pressed: true,
                        released: false,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            &mut services,
        )
        .unwrap();
    assert_eq!(frame.update, 1);
    assert!(frame.hud_holds.intro);
    assert_eq!(
        lifecycle.take_command_events(),
        vec![crate::battle::command::Event::Opened { actor: actor_id }]
    );
    assert!(lifecycle.command_frame().is_none());
    assert!(lifecycle.take_command_repeat_reset());
    let initialized = lifecycle
        .step(&mut battle, Input::default(), &mut services)
        .unwrap();
    assert_eq!(battle.ledger().elapsed_ticks, 0);
    assert!(initialized.hud_holds.intro);
    assert_eq!(lifecycle.command_frame().unwrap().animation, 1);
    assert!(lifecycle.take_command_repeat_reset());
}

#[test]
fn command_visit_failure_discards_owner_events_and_encounter_tasks() {
    let prepared = Arc::new(
        PreparedBattle::new(
            vec![actor(Side::Party, 100), actor(Side::Enemy, 100)],
            vec![],
            1,
            vec![],
            vec![],
        )
        .unwrap(),
    );
    let id = prepared.actor_ids().next().unwrap();
    let mut battle = Battle::new(prepared);
    let mut lifecycle = lifecycle(SOURCE);
    lifecycle.command_setup = Some(command::Setup {
        actors: vec![id],
        enabled: 0xdd,
    });
    let mut services = Accepted {
        fail_after_world: true,
        ..Default::default()
    };
    let result = lifecycle.step(
        &mut battle,
        Input {
            command: command::Input {
                open: ButtonInput {
                    held: true,
                    pressed: true,
                    released: false,
                },
                ..Default::default()
            },
            ..Default::default()
        },
        &mut services,
    );
    assert!(result.is_err());
    assert!(lifecycle.command_frame().is_none());
    assert!(lifecycle.take_command_events().is_empty());
    assert!(!lifecycle.take_command_repeat_reset());
    assert!(lifecycle.tasks.is_empty());
    assert_eq!(battle.phase(), BattlePhase::Finished);
}
