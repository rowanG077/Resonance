use super::*;
use crate::{BattleInput, Control, PreparedBattle, Side};

fn definition(bounce: bool, ricochet: bool) -> Arc<ProjectileDefinition> {
    Arc::new(ProjectileDefinition {
        lifetime: Some(40),
        velocity: [0., 5., 8.],
        acceleration: [0., -0.2, 0.],
        offset: [0., 75., 155.],
        clamp_ground: true,
        motion: ProjectileMotion {
            response: (bounce || ricochet).then_some(ProjectileResponse {
                bounce,
                ricochet,
                ricochet_on_deflect: false,
                ground_ricochet: false,
                restitution: 0.75,
            }),
            ..Default::default()
        },
        contact: Some(ProjectileContact {
            hit: crate::HitRule {
                element: crate::HitElement::Inherited,
                ..crate::tests::hit()
            },
            cooldown: 30,
            repeat_limit: 0,
            radius: 20.,
            height: 20.,
            shape: crate::HitShape::Box,
            offset: [0.; 3],
            radius_growth: 0.,
            height_growth: 0.,
            survives_contact: true,
            clashes: false,
        }),
        ..Default::default()
    })
}

fn instance(bounce: bool, ricochet: bool) -> Projectile {
    let mut projectile = Projectile::new(
        definition(bounce, ricochet),
        ActionId(1),
        ProjectileFrame {
            id: ProjectileId(1),
            owner: ActorId(0),
            target: ActorId(1),
            position: [0., 100., 0.],
            velocity: [0.; 3],
            heading: 0.,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        },
        3,
    );
    projectile.age = 1;
    projectile
}

#[test]
fn floor_response_bounces_or_holds_the_aimed_landing_point() {
    let mut projectile = instance(true, false);
    let ground = EffectAppearance {
        resource: crate::ResourceId(1),
        member: 3,
    };
    Arc::make_mut(&mut projectile.definition).effects.ground = Some(ground);
    projectile.frame.position = [1., 0.5, 2.];
    projectile.velocity = [3., -1., 4.];
    projectile.step().unwrap();
    assert_eq!(projectile.frame.position, [4., 0., 6.]);
    assert!((projectile.velocity[1] - 0.9).abs() < 0.000001);
    assert!(projectile.frame.contact_active);
    assert!(!projectile.frame.disarmed);
    let effects = projectile.effects();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].appearance, ground);
    assert_eq!(effects[0].origin, projectile.frame.position);
    let mut bounced_again = false;
    for _ in 0..12 {
        projectile.step().unwrap();
        bounced_again |= projectile.frame.position[1] == 0.;
        assert!(projectile.effects().is_empty());
    }
    assert!(bounced_again);

    let mut lance = instance(false, false);
    let definition = Arc::make_mut(&mut lance.definition);
    definition.lifetime = Some(60);
    definition.velocity = [0.; 3];
    definition.acceleration = [0.; 3];
    definition.offset = [0.; 3];
    definition.motion = ProjectileMotion {
        aim_at_target: true,
        stop_on_ground: true,
        speed: Some(60.),
        ..Default::default()
    };
    lance.acceleration = [0.; 3];
    lance.frame.position = [-528.80975, 300., 229.80965];
    lance.frame.heading = -90.;
    lance.target_point = [-299., 80., 0.];
    let mut random = crate::Random::new(1);
    lance
        .initialize(&mut Vec::new(), |_, _| Ok(random.next_u16()), None)
        .unwrap();
    for (actual, expected) in lance
        .velocity
        .into_iter()
        .zip([0.58556193, -0.5605664, -0.5855617])
    {
        assert!((actual - expected).abs() < 0.000001);
    }
    for _ in 0..9 {
        lance.step().unwrap();
    }
    let landing = lance.frame.position;
    assert_eq!(landing[1], 0.);
    assert!((landing[0] + 212.60623).abs() < 0.0001);
    assert_eq!(lance.velocity, [0.; 3]);
    assert!(!lance.retiring);
    while !lance.retiring {
        lance.step().unwrap();
        assert_eq!(lance.frame.position, landing);
    }
}

#[test]
fn floor_ricochet_preserves_forward_direction_until_above_ground() -> Result<()> {
    let mut projectile = instance(false, true);
    let definition = Arc::make_mut(&mut projectile.definition);
    definition.clamp_ground = false;
    definition.motion.response.as_mut().unwrap().ground_ricochet = true;
    definition.effects.spin_on_rebound = true;
    projectile.frame.position = [-173.961_75, 4.630753, 129.388_69];
    projectile.velocity = [-13., -14.5, 3.099995];
    projectile.acceleration = [0.; 3];
    projectile.step()?;
    assert!((projectile.frame.position[1] + 9.869247).abs() < 0.00001);
    for (actual, expected) in projectile
        .velocity
        .into_iter()
        .zip([-1.945452, 9., 0.463915])
    {
        assert!((actual - expected).abs() < 0.00001);
    }
    assert_eq!(projectile.acceleration, [0., -0.3, 0.]);
    assert!(projectile.ricocheted);
    assert!(!projectile.bounce);
    assert!(projectile.frame.disarmed);
    assert!(!projectile.frame.contact_active);
    assert_eq!(projectile.take_rebound(), Some(Rebound::Ground));
    let velocity = projectile.velocity;
    projectile.hit(ActorId(1), false);
    projectile.step()?;
    assert!((projectile.frame.position[1] + 0.869247).abs() < 0.00001);
    for (actual, expected) in projectile.velocity.into_iter().zip(velocity) {
        assert!((actual - expected).abs() < 0.00001);
    }
    assert!(!projectile.bounce);
    assert_eq!(projectile.take_rebound(), Some(Rebound::Ground));
    projectile.step()?;
    assert!((projectile.frame.position[1] - 8.130753).abs() < 0.00001);
    assert!((projectile.velocity[0] - velocity[0]).abs() < 0.00001);
    assert!((projectile.velocity[2] - velocity[2]).abs() < 0.00001);
    assert!((projectile.velocity[1] - 8.7).abs() < 0.00001);
    assert!(projectile.frame.disarmed);
    assert!(!projectile.frame.contact_active);
    assert_eq!(projectile.take_rebound(), None);
    Ok(())
}

#[test]
fn contact_response_disarms_once_on_the_next_update() {
    let trail = EffectAppearance {
        resource: crate::ResourceId(1),
        member: 6,
    };
    let assert_trail = |projectile: &mut Projectile| {
        let effects = projectile.effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].appearance, trail);
        assert_eq!(effects[0].origin, projectile.frame.position);
    };
    use crate::GuardResult::{Blocked, Broken, None};
    for (ricochet, guard, clash) in [
        (true, None, false),
        (true, None, true),
        (false, None, false),
        (
            false,
            Blocked {
                first: true,
                special: false,
            },
            false,
        ),
        (
            false,
            Blocked {
                first: false,
                special: true,
            },
            false,
        ),
        (false, Broken, false),
        (false, None, true),
    ] {
        let mut projectile = instance(false, true);
        let definition = Arc::make_mut(&mut projectile.definition);
        let response = definition.motion.response.as_mut().unwrap();
        response.ricochet = ricochet;
        response.ricochet_on_deflect = !ricochet;
        let effects = &mut definition.effects;
        effects.rebound_projectile = ricochet.then(|| self::definition(false, false));
        effects.trail = Some((trail, 1));
        projectile.step().unwrap();
        assert_trail(&mut projectile);
        if clash {
            projectile.clash();
        } else {
            projectile.hit(ActorId(1), guard != None);
            projectile.hit(ActorId(2), false);
        }
        assert!(projectile.take_rebound_projectile().is_none());
        let position = projectile.frame.position;
        let velocity = projectile.velocity;
        projectile.step().unwrap();
        assert_trail(&mut projectile);
        assert_eq!(
            projectile.frame.position,
            std::array::from_fn(|i| position[i] + velocity[i])
        );
        if !ricochet && guard == None && !clash {
            assert!(!projectile.frame.disarmed && projectile.frame.contact_active);
            assert_eq!(projectile.velocity[2], velocity[2]);
            assert!((projectile.velocity[1] - (velocity[1] - 0.2)).abs() < 0.00001);
            // A pending conditional response protects even a non-piercing shot.
            Arc::make_mut(&mut projectile.definition)
                .contact
                .as_mut()
                .unwrap()
                .survives_contact = false;
            while !projectile.retiring {
                projectile.step().unwrap();
            }
            assert_eq!(projectile.frame.age, 40);
            continue;
        }
        assert_eq!(projectile.velocity[0], 0.);
        assert_eq!(projectile.velocity[1], 9.);
        assert!((projectile.velocity[2] + 2.).abs() < 0.000001);
        assert_eq!(projectile.acceleration, [0., -0.3, 0.]);
        assert!(projectile.bounce);
        assert!(projectile.frame.disarmed);
        assert!(!projectile.frame.contact_active);
        assert!(!projectile.retiring);
        assert_eq!(
            projectile
                .take_rebound_projectile()
                .map(|(target, _)| target),
            (!clash && ricochet).then_some(ActorId(1))
        );
        assert!(projectile.take_rebound_projectile().is_none());
        for _ in 0..3 {
            projectile.step().unwrap();
            assert_trail(&mut projectile);
        }
        assert!(
            projectile.velocity[1] < 9.,
            "the launch impulse is applied once"
        );
        while !projectile.retiring {
            projectile.step().unwrap();
        }
        assert_eq!(
            projectile.frame.age, 40,
            "response preserves authored lifetime"
        );
    }
}

#[test]
fn contact_return_hops_toward_owner_then_departs_once() {
    let mut projectile = instance(false, false);
    let definition = Arc::make_mut(&mut projectile.definition);
    definition.motion.departure = Some(ProjectileDeparture {
        return_speed: 4.,
        jump_speed: 10.,
        ground_speed: 12.,
        ground_reference: [1., 0.],
    });
    definition.acceleration = [0., -1., 0.];
    definition.validate().unwrap();
    projectile.acceleration = definition.acceleration;
    for (owner, position, expected) in [
        ([24., 900., 33.], [20., 100., 30.], [3.2, 10., 2.4]),
        ([0.; 3], [3., 100., 4.], [-2.4, 10., -3.2]),
    ] {
        projectile.frame.position = position;
        projectile.velocity = [0., -3., 0.];
        projectile.target_point = owner;
        projectile.ricocheted = false;
        projectile.frame.disarmed = false;
        projectile.hit(ActorId(1), false);
        projectile.step().unwrap();
        assert!(!projectile.frame.contact_active && projectile.frame.disarmed);
        assert!(
            !projectile.can_hit(ActorId(2)),
            "return cannot hit another actor"
        );
        for (actual, expected) in projectile.velocity.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.00001);
        }
        projectile.target_point = [1000.; 3];
        projectile.step().unwrap();
        assert_eq!(projectile.velocity[1], 9., "the return hop runs once");
        assert!(!projectile.frame.contact_active && projectile.frame.disarmed);
    }
    // Two ordinary floor contacts and one independently translated projectile.
    for (position, expected) in [
        ([-293.208_2, 0.5, -13.746511], [-0.5600735, 0., 11.986923]),
        ([-263.0494, 0.5, -34.126984], [-1.5381423, 0., 11.901013]),
        ([336.95044, 0.5, 215.87302], [6.487067, 0., -10.095441]),
    ] {
        projectile.frame.position = position;
        projectile.velocity = [0., -1., 0.];
        projectile.departed = false;
        projectile.step().unwrap();
        assert_eq!(projectile.frame.position[1], 0.);
        for (actual, expected) in projectile.velocity.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.00002);
        }
        let direction = projectile.velocity;
        projectile.step().unwrap();
        assert_eq!(projectile.velocity, [direction[0], -1., direction[2]]);
        assert_eq!(projectile.frame.position[1], 0.);
        assert!(!projectile.frame.contact_active && projectile.frame.disarmed);
    }
}

#[test]
fn subthreshold_planar_velocity_is_scaled_without_normalization() {
    let mut projectile = instance(false, true);
    projectile.velocity = [0.05, 0., 0.];
    projectile.acceleration = [0.; 3];
    projectile.hit(ActorId(1), false);
    projectile.step().unwrap();
    assert_eq!(projectile.velocity, [-0.1, 9., 0.]);
}

#[test]
fn inactive_contact_feedback_is_retained_until_an_active_visit() {
    let mut projectile = instance(false, true);
    Arc::get_mut(&mut projectile.definition).unwrap().active = Some([3, 4]);
    projectile.hit(ActorId(1), false);
    for _ in 1..3 {
        projectile.step().unwrap();
        assert!(!projectile.frame.disarmed);
    }
    projectile.step().unwrap();
    assert!(projectile.frame.disarmed);
}

#[test]
fn menu_pause_defers_pending_ricochet_until_resume() {
    let mut owner = crate::tests::actor(Side::Party);
    owner.control = Control::Manual;
    let mut target = crate::tests::actor(Side::Enemy);
    target.control = Control::Manual;
    target.position[0] = 1000.;
    let mut battle = PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        Default::default(),
        1,
    )
    .unwrap()
    .finish()
    .unwrap();
    let mut projectile = instance(false, true);
    let mut child = definition(false, false);
    Arc::make_mut(&mut child).offset = [0.; 3];
    let effects = &mut Arc::make_mut(&mut projectile.definition).effects;
    effects.spin_on_rebound = true;
    effects.rebound_projectile = Some(child);
    projectile.hit(ActorId(1), false);
    battle.projectiles.insert(projectile.frame.id, projectile);
    battle.next_projectile = 2;
    let before = battle.projectiles[&ProjectileId(1)].frame.clone();
    let random_before = battle.random_state();
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.projectiles[&ProjectileId(1)].frame, before);
    assert_eq!(battle.random_state(), random_before);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.projectiles.len(), 1);
    assert!(battle.projectiles[&ProjectileId(1)].frame.disarmed);
    let mut expected = crate::Random::new(random_before);
    expected.next_u16();
    assert_eq!(battle.random_state(), expected.state());
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::ProjectileRebound {
            projectile: ProjectileId(1),
            first: true,
            pitch: None
        }
    )));
    let after_ricochet = frame.projectiles;
    let paused = battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.projectiles, after_ricochet);
    assert_eq!(battle.random_state(), expected.state());
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.projectiles.len(), 2);
    let child = &battle.projectiles[&ProjectileId(2)];
    assert_eq!(
        child.frame.position,
        std::array::from_fn(|axis| battle.actors[1].position[axis] + [0., 5., 8.][axis]),
        "the child begins moving on the update after ricochet"
    );
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::ProjectileStarted {
            projectile: ProjectileId(2),
            ..
        }
    )));
    let child_origin = child.frame.position;
    let child_velocity = child.velocity;
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.projectiles.len(), 2, "the child is emitted only once");
    assert_eq!(
        battle.projectiles[&ProjectileId(2)].frame.position,
        std::array::from_fn(|axis| child_origin[axis] + child_velocity[axis])
    );
    assert_eq!(battle.random_state(), expected.state());
    assert!(!frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::ProjectileRebound { .. } | Cue::ProjectileStarted { .. }
    )));
}
