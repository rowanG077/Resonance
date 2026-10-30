use super::*;

pub(crate) fn tables() -> CollisionSettings {
    CollisionSettings {
        radius: 1200.,
        ship_probe_half_extent: 120.,
        ground_probe_half_extent: 50.,
    }
}
pub(crate) fn rectangle(
    surface: TerrainSurface,
    min: [f32; 2],
    max: [f32; 2],
    height: f32,
) -> CollisionGroup {
    CollisionGroup {
        surface: surface.index() as u32,
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
    tables: &'a CollisionSettings,
    origin: [f32; 3],
) -> Query<'a> {
    Query {
        faces: Mesh::new(groups).unwrap().faces,
        settings: tables,
        origin,
    }
}

#[test]
fn ship_rejects_land_overlapping_water_in_either_source_order() {
    let tables = tables();
    let water = rectangle(TerrainSurface::Water, [-500.; 2], [500.; 2], 0.);
    let land = rectangle(TerrainSurface::Grassland, [-100.; 2], [100.; 2], 40.);
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
    let water = rectangle(TerrainSurface::Water, [-100.; 2], [100.; 2], 0.);
    let ground = rectangle(TerrainSurface::Grassland, [-100.; 2], [100.; 2], 0.);
    assert!(!query(&[water], &tables, [0.; 3]).has_clearance(Mode::Ship));
    assert!(query(std::slice::from_ref(&ground), &tables, [0.; 3]).has_clearance(Mode::Ground));
    assert!(!query(&[ground], &tables, [51., 0., 0.]).has_clearance(Mode::Ground));
}

#[test]
fn first_walkable_face_keeps_its_surface_and_barriers_keep_flight_clearance() {
    let tables = tables();
    let forest = rectangle(TerrainSurface::DeepForest, [-500.; 2], [500.; 2], 25.);
    let grass = rectangle(TerrainSurface::Grassland, [-500.; 2], [500.; 2], 80.);
    let barrier = rectangle(TerrainSurface::Barrier, [-500.; 2], [500.; 2], 300.);
    let groups = [barrier, forest, grass];
    let q = query(&groups, &tables, [0.; 3]);
    let surface = q.surface(Mode::Ground).unwrap();
    assert_eq!(
        (surface.kind, surface.height),
        (TerrainSurface::DeepForest, 25.)
    );
    assert_eq!(q.flight_floor(), 300.);
    assert!(q.surface(Mode::Ship).is_none());
}

#[test]
fn blocked_motion_tries_positive_then_negative_sixty_degrees() -> Result<()> {
    let tables = tables();
    let right = rectangle(TerrainSurface::Grassland, [20., -180.], [300., 100.], 0.);
    let left = rectangle(TerrainSurface::Road, [-300., -180.], [-20., 100.], 0.);
    let groups = [right, left.clone()];
    let motion = query(&groups, &tables, [0.; 3]).motion(100., 0., Mode::Ground)?;
    assert_eq!(motion.surface, Some(TerrainSurface::Grassland));
    assert!(motion.delta[0] > 85. && motion.delta[1] < -49.);
    let motion = query(&[left], &tables, [0.; 3]).motion(100., 0., Mode::Ground)?;
    assert_eq!(motion.surface, Some(TerrainSurface::Road));
    assert!(motion.delta[0] < -85.);
    Ok(())
}

#[test]
fn slope_correction_projects_onto_the_plane_and_invalid_input_is_rejected() -> Result<()> {
    let tables = tables();
    let mut ramp = rectangle(TerrainSurface::Grassland, [-500.; 2], [500.; 2], 0.);
    for vertex in &mut ramp.vertices {
        vertex[2] = vertex[0] + 100.;
    }
    let groups = [ramp];
    let q = query(&groups, &tables, [0.; 3]);
    assert_eq!(q.surface(Mode::Ground).unwrap().height, 100.);
    assert_eq!(q.flight_floor(), 100.);
    let motion = q.motion(0., 0., Mode::Ground)?;
    assert!((motion.delta[2] - 50.).abs() < 0.0001);
    assert!((motion.slope[1] + std::f32::consts::FRAC_PI_4).abs() < 0.0001);
    assert!(q.motion(f32::NAN, 0., Mode::Ground).is_err());
    assert!(q.motion(1., f32::INFINITY, Mode::Ground).is_err());
    let motion = q.motion(1000., 0., Mode::Ground)?;
    assert_eq!(motion.surface, None);
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
                Mesh::new(&[rectangle(
                    TerrainSurface::Grassland,
                    [2900., -300.],
                    [3200., 300.],
                    0.,
                )])?,
            ),
            (
                east,
                Mesh::new(&[rectangle(
                    TerrainSurface::Grassland,
                    [-3200., -300.],
                    [-2900., 300.],
                    0.,
                )])?,
            ),
        ],
        tables(),
    )?;
    let motion =
        terrain
            .query(origin, 500.)?
            .motion(40., std::f32::consts::FRAC_PI_2, Mode::Ground)?;
    assert_eq!(motion.surface, Some(TerrainSurface::Grassland));
    let next = origin.translated(motion.delta)?;
    assert_eq!(next.tile(), east);
    assert!((next.map()[0] - 30.).abs() < 1.);
    let incomplete = Terrain::new([(west, Mesh::new(&[])?)], tables())?;
    assert!(incomplete.query(origin, 500.).is_err());
    Ok(())
}

#[test]
fn malformed_meshes_and_tables_fail_preparation() {
    let mut group = rectangle(TerrainSurface::Grassland, [-100.; 2], [100.; 2], 0.);
    group.triangles[0][0] = 100;
    assert!(Mesh::new(&[group]).is_err());
    let mut invalid = rectangle(TerrainSurface::Grassland, [-100.; 2], [100.; 2], 0.);
    invalid.surface = 16;
    assert!(Mesh::new(&[invalid]).is_err());
    let mut invalid = tables();
    invalid.ground_probe_half_extent = f32::INFINITY;
    assert!(invalid.validate().is_err());
}
