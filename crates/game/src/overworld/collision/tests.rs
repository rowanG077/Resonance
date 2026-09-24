use super::*;

pub(crate) fn tables() -> CollisionTables {
    CollisionTables {
        surface_responses: [
            [-1, -1, -1, -1],
            [1, 1, -1, 1],
            [2, 2, -1, 2],
            [3, 3, -1, 3],
            [4, 4, -1, 4],
            [5, 5, -1, 5],
            [6, 6, -1, -1],
            [7, 1, -1, -1],
            [-1, -1, -1, -1],
            [-1, -1, -1, -1],
            [10, -1, -1, 1],
            [-1, -1, 11, -1],
            [-1, -1, -1, -1],
            [13, 13, -1, -1],
            [-1, -1, -1, -1],
            [15, -1, -1, -1],
        ],
        mode2_probe_half_extent: 120.,
        other_probe_half_extent: 50.,
    }
}
pub(crate) fn rectangle(surface: u32, min: [f32; 2], max: [f32; 2], height: f32) -> CollisionGroup {
    CollisionGroup {
        surface,
        vertices: vec![
            [min[0], min[1], height],
            [max[0], min[1], height],
            [max[0], max[1], height],
            [min[0], max[1], height],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}
fn query<'a>(
    groups: &[CollisionGroup],
    tables: &'a CollisionTables,
    origin: [f32; 3],
) -> Query<'a> {
    Query {
        faces: Mesh::new(groups).unwrap().faces,
        tables,
        origin,
    }
}

#[test]
fn ship_rejects_land_overlapping_water_in_either_source_order() {
    let tables = tables();
    let water = rectangle(11, [-500.; 2], [500.; 2], 0.);
    let land = rectangle(1, [-100.; 2], [100.; 2], 40.);
    for groups in [
        vec![water.clone(), land.clone()],
        vec![land.clone(), water.clone()],
    ] {
        let q = query(&groups, &tables, [0.; 3]);
        assert!(q.surface(Mode::Ship).is_none());
        assert_eq!(q.surface(Mode::Ground).unwrap().height, 40.);
        assert!(q.face([300., 0., 0.], Mode::Ship).is_some());
    }
}

#[test]
fn probes_require_all_four_corners_with_the_modes_own_extent() {
    let tables = tables();
    let water = rectangle(11, [-100.; 2], [100.; 2], 0.);
    let ground = rectangle(1, [-100.; 2], [100.; 2], 0.);
    assert!(!query(&[water], &tables, [0.; 3]).has_clearance(Mode::Ship));
    assert!(query(std::slice::from_ref(&ground), &tables, [0.; 3]).has_clearance(Mode::Ground));
    assert!(!query(&[ground], &tables, [51., 0., 0.]).has_clearance(Mode::Ground));
}

#[test]
fn first_eligible_face_and_response_class_are_preserved() {
    let tables = tables();
    let forest = rectangle(7, [-500.; 2], [500.; 2], 25.);
    let grass = rectangle(1, [-500.; 2], [500.; 2], 80.);
    let groups = [forest, grass];
    let q = query(&groups, &tables, [0.; 3]);
    let surface = q.surface(Mode::AlternateGround).unwrap();
    assert_eq!(
        (surface.surface, surface.response, surface.height),
        (7, 1, 25.)
    );
    assert_eq!(q.surface(Mode::Ground).unwrap().response, 7);
    assert_eq!(q.surface(Mode::RestrictedGround).unwrap().height, 80.);
}

#[test]
fn blocked_motion_tries_positive_then_negative_sixty_degrees() -> Result<()> {
    let tables = tables();
    let right = rectangle(1, [20., -180.], [300., 100.], 0.);
    let left = rectangle(2, [-300., -180.], [-20., 100.], 0.);
    let groups = [right, left.clone()];
    let motion = query(&groups, &tables, [0.; 3]).motion(100., 0., Mode::Ground)?;
    assert_eq!(motion.response, Some(1));
    assert!(motion.delta[0] > 85. && motion.delta[1] < -49.);
    let motion = query(&[left], &tables, [0.; 3]).motion(100., 0., Mode::Ground)?;
    assert_eq!(motion.response, Some(2));
    assert!(motion.delta[0] < -85.);
    Ok(())
}

#[test]
fn slope_correction_projects_onto_the_plane_and_invalid_input_is_rejected() -> Result<()> {
    let tables = tables();
    let mut ramp = rectangle(1, [-500.; 2], [500.; 2], 0.);
    for vertex in &mut ramp.vertices {
        vertex[2] = vertex[0] + 100.;
    }
    let groups = [ramp];
    let q = query(&groups, &tables, [0.; 3]);
    assert_eq!(q.surface(Mode::Ground).unwrap().height, 100.);
    let motion = q.motion(0., 0., Mode::Ground)?;
    assert!((motion.delta[2] - 50.).abs() < 0.0001);
    assert!((motion.slope[1] + std::f32::consts::FRAC_PI_4).abs() < 0.0001);
    assert!(q.motion(f32::NAN, 0., Mode::Ground).is_err());
    assert!(q.motion(1., f32::INFINITY, Mode::Ground).is_err());
    let motion = q.motion(1000., 0., Mode::Ground)?;
    assert_eq!(motion.response, None);
    assert_eq!(&motion.delta[..2], [0., 0.]);
    assert!((motion.delta[2] - 50.).abs() < 0.0001);
    Ok(())
}

#[test]
fn movement_crosses_the_world_seam_using_the_adjacent_tile() -> Result<()> {
    let west = TileCoordinate::new(11, 0)?;
    let east = TileCoordinate::new(0, 0)?;
    let origin = Position::from_map([76790., 3200., 0.])?;
    let terrain = Terrain::new(
        [
            (
                west,
                Mesh::new(&[rectangle(1, [2900., -300.], [3200., 300.], 0.)])?,
            ),
            (
                east,
                Mesh::new(&[rectangle(1, [-3200., -300.], [-2900., 300.], 0.)])?,
            ),
        ],
        tables(),
    )?;
    let motion =
        terrain
            .query(origin, 500.)?
            .motion(40., std::f32::consts::FRAC_PI_2, Mode::Ground)?;
    assert_eq!(motion.response, Some(1));
    let next = origin.translated(motion.delta)?;
    assert_eq!(next.tile(), east);
    assert!((next.map()[0] - 30.).abs() < 1.);
    let incomplete = Terrain::new([(west, Mesh::new(&[])?)], tables())?;
    assert!(incomplete.query(origin, 500.).is_err());
    Ok(())
}

#[test]
fn malformed_meshes_and_tables_fail_preparation() {
    let mut group = rectangle(1, [-100.; 2], [100.; 2], 0.);
    group.triangles[0][0] = 100;
    assert!(Mesh::new(&[group]).is_err());
    assert!(Mesh::new(&[rectangle(16, [-100.; 2], [100.; 2], 0.)]).is_err());
    let mut invalid = tables();
    invalid.surface_responses[0][0] = -2;
    assert!(invalid.validate().is_err());
    let mut invalid = tables();
    invalid.other_probe_half_extent = f32::INFINITY;
    assert!(invalid.validate().is_err());
}
