//! Scheduling/ownership witnesses using original common-3 particle input. These
//! do not substitute common-3 for the original Lightning-28 or clash-11 visuals.
use super::{actor, effect_runtime::compiled};
use resonance_battle::{
    ActionDefinition, ActionPhase, ActionRequest, Battle, BattleInput, Cue, EffectAppearance,
    EffectBank, PreparedBattle, ProjectileContact, ProjectileDefinition, ResourceBinding, Side,
};
use std::sync::Arc;

fn projectile() -> ProjectileDefinition {
    ProjectileDefinition {
        motion: Default::default(),
        effects: Default::default(),
        lifetime: 20,
        velocity: [0.; 3],
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: false,
        active: None,
        birth: None,
        contact: None,
    }
}

fn start(
    definitions: Vec<ProjectileDefinition>,
    seed: u32,
) -> (Battle, Vec<resonance_battle::ActorId>) {
    let (program, entry) = compiled(
        "script battle; use battle; asset p: battle::Projectile = \"test/projectile\"; pub task run() { battle::emit(p, battle::ground_point(battle::target(), 0.0, 0.0)); battle::finish(); }",
    );
    let mut owner = actor();
    owner.position = [10., 0., 0.];
    owner.hp = 10000;
    owner.max_hp = 10000;
    owner.stats.slash = 1200;
    owner.stats.accuracy = 80;
    owner.body.points = vec![resonance_battle::HurtPoint {
        center: owner.position,
        radius: 1.,
    }];
    let mut enemy = owner.clone();
    enemy.side = Side::Enemy;
    let actions: Vec<_> = definitions
        .into_iter()
        .enumerate()
        .map(|(i, p)| ActionDefinition {
            id: i as u16 + 1,
            phase: ActionPhase::Resident,
            program: program.clone(),
            entry,
            duration: 20,
            tp_cost: 0,
            resources: vec![ResourceBinding::Projectile(Arc::new(p))],
        })
        .collect();
    let count = actions.len();
    let prepared = Arc::new(
        PreparedBattle::new(
            vec![owner, enemy],
            actions,
            seed,
            vec![],
            vec![EffectBank {
                models: Default::default(),
                resource: 77,
                members: super::effect_runtime::definitions(),
            }],
        )
        .unwrap(),
    );
    let ids: Vec<_> = prepared.actor_ids().collect();
    let mut battle = Battle::new(prepared);
    let frame = battle
        .step(BattleInput {
            actions: (0..count)
                .map(|i| ActionRequest {
                    actor: ids[i],
                    target: ids[i],
                    action: i as u16 + 1,
                })
                .collect(),
            ..Default::default()
        })
        .unwrap();
    assert!(frame.particles.is_empty());
    assert_eq!(battle.random_state(), seed);
    (battle, ids)
}

#[test]
fn birth_constructs_before_motion_follows_each_visit_and_survives_projectile_retirement() {
    let mut p = projectile();
    p.birth = Some(EffectAppearance {
        resource: 77,
        member: 3,
    });
    p.velocity[0] = 3.;
    p.offset[0] = 2.;
    p.lifetime = 1;
    let (mut battle, _) = start(vec![p], 1);
    let first = battle.step(BattleInput::default()).unwrap();
    assert_eq!(first.projectiles[0].position, [15., 0., 0.]);
    assert_eq!(first.particles.len(), 3);
    assert!(first.particles.iter().all(|p| p.origin == [12., 0., 0.]));
    let begun = first
        .cues
        .iter()
        .position(|c| matches!(c, Cue::ProjectileStarted { .. }))
        .unwrap();
    let effect = first
        .cues
        .iter()
        .position(|c| matches!(c, Cue::Effect { .. }))
        .unwrap();
    let particle = first
        .cues
        .iter()
        .position(|c| matches!(c, Cue::ParticleStarted { .. }))
        .unwrap();
    assert!(begun < effect && effect < particle);
    let state = battle.random_state();
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.particles, first.particles);
    assert_eq!(battle.random_state(), state);
    let last = battle.step(BattleInput::default()).unwrap();
    assert_eq!(last.projectiles[0].position, [18., 0., 0.]);
    assert_eq!(last.particles.last().unwrap().origin, [18., 0., 0.]);
    let retired = battle.step(BattleInput::default()).unwrap();
    assert!(retired.projectiles.is_empty());
    let tail = battle.step(BattleInput::default()).unwrap();
    assert_eq!(tail.particles.len(), 5);
    assert_eq!(tail.particles.last().unwrap().origin, [18., 0., 0.]);
    assert!(
        tail.particles
            .iter()
            .take(3)
            .all(|p| p.origin == [12., 0., 0.])
    );
}

fn contact(clash: bool) -> ProjectileContact {
    ProjectileContact {
        hit: resonance_battle::HitRule {
            impact: None,
            arte: false,
            reaction: Default::default(),
            kind: resonance_battle::DamageKind::Slash,
            power: resonance_battle::Power::Normal,
            element: resonance_battle::HitElement::Neutral,
            prevents_defeat: false,
            guard: Default::default(),
        },
        cooldown: 120,
        repeat_limit: 0,
        radius: 10.,
        height: 10.,
        shape: resonance_battle::HitShape::Sphere,
        offset: [0.; 3],
        radius_growth: 0.,
        height_growth: 0.,
        survives_contact: true,
        clash_effect: clash.then_some(EffectAppearance {
            resource: 77,
            member: 3,
        }),
    }
}

#[test]
fn clash_particle_rng_executes_before_the_next_contact_damage() {
    let observations: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/particle-emissions.json")).unwrap();
    let native = &observations["constructors"][0];
    let before = native["random_before"].as_u64().unwrap() as u32;
    let after = native["random_after"].as_u64().unwrap() as u32;
    let mut party = projectile();
    party.contact = Some(contact(true));
    let mut enemy = projectile();
    enemy.contact = Some(contact(false));
    let (mut with_effect, _) = start(vec![party, enemy.clone()], before);
    with_effect.step(BattleInput::default()).unwrap(); // birth, no contacts
    let frame = with_effect.step(BattleInput::default()).unwrap();
    let hit = |frame: &resonance_battle::BattleFrame| {
        frame
            .cues
            .iter()
            .find_map(|c| match c {
                Cue::Hit { result, .. } => Some(*result),
                _ => None,
            })
            .unwrap()
    };
    let reference = |seed| {
        let (mut battle, _) = start(vec![projectile(), enemy.clone()], seed);
        battle.step(BattleInput::default()).unwrap();
        let frame = battle.step(BattleInput::default()).unwrap();
        (hit(&frame), battle.random_state())
    };
    let expected = reference(after);
    assert_eq!((hit(&frame), with_effect.random_state()), expected);
    assert_ne!(
        hit(&frame),
        reference(before).0,
        "witness must detect late effect execution"
    );
    // Contacts run after the particle group. Constructor RNG is immediate,
    // but the new particles first update/draw on the following simulation step.
    assert!(frame.particles.is_empty());
    let effect = frame
        .cues
        .iter()
        .position(|c| matches!(c, Cue::Effect { .. }))
        .unwrap();
    let damage = frame
        .cues
        .iter()
        .position(|c| matches!(c, Cue::Hit { .. }))
        .unwrap();
    assert!(effect < damage);
    let next = with_effect.step(BattleInput::default()).unwrap();
    assert_eq!(next.particles.len(), 3);
    assert!(next.particles.iter().all(|p| p.origin == [10., 0., 0.]));
}
