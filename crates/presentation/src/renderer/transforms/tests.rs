use super::*;
use std::collections::HashSet;

fn app() -> (App, [Entity; 6]) {
    let mut app = App::new();
    app.add_plugins((bevy::app::TaskPoolPlugin::default(), SerialTransformPlugin));
    let world = app.world_mut();
    let root = world
        .spawn(
            Transform::from_xyz(4., 2., -3.)
                .with_rotation(Quat::from_rotation_z(0.7))
                .with_scale(Vec3::new(2., 3., -1.)),
        )
        .id();
    let other = world.spawn(Transform::from_xyz(-6., 8., 2.)).id();
    let branch = world
        .spawn((
            Transform::from_rotation(Quat::from_rotation_x(0.4)),
            ChildOf(root),
        ))
        .id();
    let leaf = world
        .spawn((Transform::from_xyz(1., 2., 3.), ChildOf(branch)))
        .id();
    let sibling = world
        .spawn((Transform::from_xyz(3., 4., 5.), ChildOf(root)))
        .id();
    let standalone = world.spawn(Transform::from_xyz(9., 1., 2.)).id();
    (app, [root, other, branch, leaf, sibling, standalone])
}

fn assert_pose(world: &World, entity: Entity) {
    let mut expected = GlobalTransform::IDENTITY;
    let mut node = Some(entity);
    while let Some(entity) = node {
        expected = GlobalTransform::from(*world.get::<Transform>(entity).unwrap()) * expected;
        node = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    let actual = world.get::<GlobalTransform>(entity).unwrap().affine();
    assert!(
        actual.abs_diff_eq(expected.affine(), 0.0001),
        "incorrect world transform for {entity:?}"
    );
}

fn update_and_check(app: &mut App) {
    app.update();
    let mut entities = app.world_mut().query_filtered::<Entity, With<Transform>>();
    for entity in entities.iter(app.world()) {
        assert_pose(app.world(), entity);
    }
}

#[test]
fn hierarchy_edits_propagate_with_and_without_static_optimizations() {
    use StaticTransformOptimizations::{Disabled, Enabled};

    for optimizations in [Enabled, Disabled] {
        let (mut app, [root, other, branch, leaf, sibling, _]) = app();
        app.insert_resource(optimizations);
        update_and_check(&mut app);

        app.world_mut()
            .get_mut::<Transform>(root)
            .unwrap()
            .translation
            .x += 5.;
        update_and_check(&mut app);
        app.world_mut().get_mut::<Transform>(leaf).unwrap().scale = Vec3::ZERO;
        update_and_check(&mut app);

        app.world_mut().entity_mut(branch).insert(ChildOf(other));
        update_and_check(&mut app);
        app.world_mut().entity_mut(branch).remove::<ChildOf>();
        update_and_check(&mut app);
        app.world_mut().entity_mut(leaf).remove::<ChildOf>();
        update_and_check(&mut app);

        let world = app.world_mut();
        world.entity_mut(sibling).remove::<ChildOf>();
        world.entity_mut(sibling).insert(ChildOf(other));
        update_and_check(&mut app);
        let world = app.world_mut();
        world.entity_mut(sibling).remove::<GlobalTransform>();
        world.entity_mut(sibling).insert(GlobalTransform::IDENTITY);
        update_and_check(&mut app);

        app.world_mut()
            .spawn((Transform::from_xyz(1., 3., 5.), ChildOf(branch)));
        update_and_check(&mut app);
        app.world_mut()
            .get_mut::<Transform>(branch)
            .unwrap()
            .rotation = Quat::from_rotation_y(1.2);
        update_and_check(&mut app);
    }
}

#[derive(Resource, Default)]
struct ChangedGlobals(HashSet<Entity>);

#[test]
fn animated_leaf_leaves_unchanged_branches_clean() {
    let (mut app, [root, other, branch, leaf, sibling, standalone]) = app();
    app.init_resource::<ChangedGlobals>().add_systems(
        PostUpdate,
        (|query: Query<Entity, Changed<GlobalTransform>>, mut changed: ResMut<ChangedGlobals>| {
            changed.0.clear();
            changed.0.extend(query.iter());
        })
        .after(TransformSystems::Propagate),
    );
    app.update();
    app.update();
    assert!(app.world().resource::<ChangedGlobals>().0.is_empty());
    app.world_mut()
        .get_mut::<Transform>(leaf)
        .unwrap()
        .translation
        .x += 1.;
    app.update();
    let changed = &app.world().resource::<ChangedGlobals>().0;
    assert!(changed.contains(&leaf));
    for entity in [root, other, branch, sibling, standalone] {
        assert!(!changed.contains(&entity));
    }
    assert_pose(app.world(), leaf);
}

#[test]
fn startup_and_affine_animation_keep_their_propagation_order() {
    use crate::sparse_animation::affine::{self, Locals, Pose};
    use bevy::math::{Affine3A, Mat3};

    let (mut app, [root, _, branch, leaf, _, _]) = app();
    app.add_systems(
        PostStartup,
        (move |world: &mut World| assert_pose(world, leaf)).after(TransformSystems::Propagate),
    );
    affine::install(&mut app);
    let matrix = Affine3A::from_mat3_translation(
        Mat3::from_cols(Vec3::X * 2., Vec3::new(0.5, 3., 0.), Vec3::Z),
        Vec3::new(7., 8., 9.),
    );
    app.add_systems(
        Update,
        move |mut locals: ResMut<Locals>, mut transforms: Query<&mut Transform>| {
            locals.set(
                root,
                &mut transforms.get_mut(root).unwrap(),
                Pose::Affine(matrix),
            );
        },
    );
    for _ in 0..2 {
        app.update();
        let world = app.world();
        let expected = GlobalTransform::from(matrix)
            * GlobalTransform::from(*world.get::<Transform>(branch).unwrap())
            * GlobalTransform::from(*world.get::<Transform>(leaf).unwrap());
        let actual = world.get::<GlobalTransform>(leaf).unwrap().affine();
        assert!(actual.abs_diff_eq(expected.affine(), 0.0001));
    }
}

#[test]
fn integrated_affine_walk_matches_attachment_queries_and_releases_overrides() {
    use crate::sparse_animation::affine::{self, Helper, Locals, Pose};
    use bevy::{ecs::system::RunSystemOnce, math::Affine3A};

    #[derive(Resource)]
    struct Frame(Option<u8>);

    for optimizations in [
        StaticTransformOptimizations::Enabled,
        StaticTransformOptimizations::Disabled,
    ] {
        let (mut app, entities @ [root, _, branch, leaf, _, standalone]) = app();
        affine::install(&mut app);
        app.insert_resource(optimizations)
            .insert_resource(Frame(None));
        // A child's translation boundary can reveal a base movement that its
        // parent's own offset cancels. The parent's GlobalTransform stays equal.
        *app.world_mut().get_mut::<Transform>(root).unwrap() = Transform::IDENTITY;
        app.add_systems(
            Update,
            move |frame: Res<Frame>,
                  mut locals: ResMut<Locals>,
                  mut nodes: Query<&mut Transform>| {
                let Some(frame) = frame.0 else { return };
                if frame < 2 {
                    let x = 10. * (f32::from(frame) + 1.);
                    locals.set_world(root, GlobalTransform::from_translation(Vec3::X * x));
                    locals.translate(root, -Vec3::X * x);
                    locals.translation_boundary(branch);
                } else {
                    let matrix = Affine3A::from_cols(
                        Vec3::ZERO.into(),
                        Vec3::new(0.5, 2., 0.).into(),
                        (-Vec3::Z).into(),
                        Vec3::new(3., 4., 5.).into(),
                    );
                    locals.set(
                        root,
                        &mut nodes.get_mut(root).unwrap(),
                        Pose::Affine(matrix),
                    );
                    locals.set_world(leaf, GlobalTransform::from_translation(Vec3::Y * 90.));
                    locals.translate(leaf, Vec3::Z * 7.);
                    locals.set(
                        standalone,
                        &mut nodes.get_mut(standalone).unwrap(),
                        Pose::Affine(matrix),
                    );
                }
            },
        );
        for frame in [None, Some(0), Some(1), Some(2), None] {
            app.world_mut().resource_mut::<Frame>().0 = frame;
            app.update();
            app.world_mut()
                .run_system_once(move |helper: Helper, globals: Query<&GlobalTransform>| {
                    let expected = helper.compute_global_transforms(entities).unwrap();
                    for (entity, expected) in entities.into_iter().zip(expected) {
                        assert!(
                            globals
                                .get(entity)
                                .unwrap()
                                .affine()
                                .abs_diff_eq(expected.affine(), 0.0001),
                            "incorrect integrated pose for {entity:?}, frame {frame:?}"
                        );
                    }
                })
                .unwrap();
            if frame.is_none() {
                for entity in entities {
                    assert_pose(app.world(), entity);
                }
            }
        }
    }
}
