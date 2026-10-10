use super::*;
use crate::{BattleInput, Control, PreparedBattle, Side};

fn definition(bounce: bool, ricochet: bool) -> Arc<ProjectileDefinition> {
    Arc::new(ProjectileDefinition {
        lifetime: Some(40),
        velocity: [0., 5., 8.],
        acceleration: [0., -0.2, 0.],
        offset: [0., 75., 155.],
        clamp_ground: true,
        active: None,
        birth: None,
        motion: ProjectileMotion {
            response: Some(ProjectileResponse {
                bounce,
                ricochet,
                restitution: 0.75,
            }),
            ..Default::default()
        },
        effects: Default::default(),
        contact: Some(ProjectileContact {
            hit: crate::HitRule {
                kind: crate::DamageKind::Slash,
                arte: false,
                overlimit_pause: false,
                power: crate::Power::Normal,
                element: crate::HitElement::Inherited,
                prevents_defeat: false,
                guard: Default::default(),
                reaction: Default::default(),
                condition: None,
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
            heading: 0.,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        },
    );
    projectile.age = 1;
    projectile
}

#[test]
fn ground_bounce_restores_upward_velocity_and_preserves_planar_motion() {
    let mut projectile = instance(true, false);
    projectile.frame.position = [1., 0.5, 2.];
    projectile.velocity = [3., -1., 4.];
    projectile.step().unwrap();
    assert_eq!(projectile.frame.position, [4., 0., 6.]);
    assert!((projectile.velocity[1] - 0.9).abs() < 0.000001);
    assert!(projectile.frame.contact_active);
    assert!(!projectile.frame.disarmed);
}

#[test]
fn contact_response_disarms_once_on_the_next_update() {
    for clash in [false, true] {
        let mut projectile = instance(false, true);
        projectile.step().unwrap();
        if clash {
            projectile.clash();
        } else {
            projectile.hit(ActorId(1));
        }
        let position = projectile.frame.position;
        let velocity = projectile.velocity;
        projectile.step().unwrap();
        assert_eq!(
            projectile.frame.position,
            std::array::from_fn(|i| position[i] + velocity[i])
        );
        assert_eq!(projectile.velocity[0], 0.);
        assert_eq!(projectile.velocity[1], 9.);
        assert!((projectile.velocity[2] + 2.).abs() < 0.000001);
        assert_eq!(projectile.acceleration, [0., -0.3, 0.]);
        assert!(projectile.bounce);
        assert!(projectile.frame.disarmed);
        assert!(!projectile.frame.contact_active);
        assert!(!projectile.retiring);
        for _ in 0..3 {
            projectile.step().unwrap();
        }
        assert!(
            projectile.velocity[1] < 9.,
            "the launch impulse is applied once"
        );
    }
}

#[test]
fn subthreshold_planar_velocity_is_scaled_without_normalization() {
    let mut projectile = instance(false, true);
    projectile.velocity = [0.05, 0., 0.];
    projectile.acceleration = [0.; 3];
    projectile.hit(ActorId(1));
    projectile.step().unwrap();
    assert_eq!(projectile.velocity, [-0.1, 9., 0.]);
}

#[test]
fn inactive_contact_feedback_is_retained_until_an_active_visit() {
    let mut projectile = instance(false, true);
    Arc::get_mut(&mut projectile.definition).unwrap().active = Some([3, 4]);
    projectile.hit(ActorId(1));
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
    projectile.hit(ActorId(1));
    battle.projectiles.insert(projectile.frame.id, projectile);
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
    assert!(frame.projectiles[0].disarmed);
}
