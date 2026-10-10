//! Draw-only particle geometry. Motion, palette selection and lifetime come from
//! the authoritative frame; drawing never advances a particle or consumes RNG.
use anyhow::{Result, bail, ensure};
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};
use resonance_battle::{ParticleFrame, ParticleGeometry, ParticleState};
use resonance_content::battle_effect::{
    declaration::Declaration,
    visual::{Orientation, ParticleShape, ParticleVisual, UvLayout},
};

const DEGREES: f32 = std::f32::consts::PI / 180.;

const SHADOW_SEGMENTS: usize = 16;

/// Batch ground footprints as soft discs, without a texture or a draw per shadow.
pub(super) fn shadows(shadows: impl Iterator<Item = ([f32; 3], f32, [u8; 4])>) -> Option<Mesh> {
    let mut vertices = Vertices::default();
    for (position, radius, rgba) in shadows {
        let center = Vec3::from_array(position);
        let mut color = rgba.map(|v| f32::from(v) / 255.);
        color[3] *= 0.4; // A soft ground shadow, fading to transparent at the edge.
        let mut edge = color;
        edge[3] = 0.;
        let point = |index: usize| {
            let angle =
                (index % SHADOW_SEGMENTS) as f32 * std::f32::consts::TAU / SHADOW_SEGMENTS as f32;
            let (sin, cos) = angle.sin_cos();
            center + Vec3::new(radius * cos, 0., -radius * sin)
        };
        for segment in 0..SHADOW_SEGMENTS {
            vertices.triangle(
                [point(segment), point(segment + 1), center],
                [edge, edge, color],
                [[0.; 2]; 3],
            );
        }
    }
    (!vertices.positions.is_empty()).then(|| vertices.mesh())
}

pub(super) fn validate(declaration: &Declaration) -> Result<()> {
    if let Declaration::ModelParticle { visual, .. } = declaration {
        ensure!(
            visual.blend <= 1,
            "model particle blend {} is unsupported",
            visual.blend
        );
        return Ok(());
    }
    let visual = declaration.particle_visual()?;
    let template = declaration.template()?;
    ensure!(
        visual.blend <= 2,
        "particle blend {} is unsupported",
        visual.blend
    );
    match &visual.geometry {
        ParticleShape::Unsupported { reason } => bail!("{reason}"),
        ParticleShape::Ring { segments, .. }
        | ParticleShape::Disc { segments }
        | ParticleShape::Shell { segments, .. } => {
            ensure!(*segments > 0, "particle has no geometry segments");
        }
        ParticleShape::Sphere { columns } => ensure!(*columns > 0, "sphere has no columns"),
        ParticleShape::VertexQuad { copy_axis, .. } => {
            ensure!(*copy_axis < 3, "invalid particle copy axis")
        }
        ParticleShape::Spiral {
            radius_step,
            segment_offset,
            steps_per_segment,
            ..
        } => {
            ensure!(
                *steps_per_segment > 0
                    && radius_step.is_finite()
                    && segment_offset.iter().all(|v| v.is_finite()),
                "invalid spiral geometry"
            );
        }
        _ => {}
    }
    if let ParticleShape::Ring { uv_layout, .. }
    | ParticleShape::Shell { uv_layout, .. }
    | ParticleShape::Spiral { uv_layout, .. } = &visual.geometry
        && let UvLayout::AlternatingHalves { panels_per_half } = uv_layout
    {
        ensure!(
            *panels_per_half > 0,
            "particle UV layout has no panels per half"
        );
    }
    if matches!(
        visual.geometry,
        ParticleShape::Orbit | ParticleShape::BillboardTrail | ParticleShape::Spiral { .. }
    ) {
        ensure!(
            template.state.geometry_count > 0,
            "particle has no geometry segments"
        );
    }
    Ok(())
}

pub(super) fn mesh(
    particle: &ParticleFrame,
    declaration: &Declaration,
    texture_size: [u32; 2],
    camera: Mat3,
) -> Result<Option<Mesh>> {
    let mut data = geometry(
        &particle.state,
        declaration,
        particle.heading,
        texture_size,
        camera,
    )?;
    if let Some(data) = &mut data {
        let mut origin =
            Vec3::from_array(particle.origin) + Vec3::from_array(particle.state.offset);
        // Ground-relative sprites ignore emitter height.
        if declaration.particle_visual()?.ground_relative {
            origin.y = 0.1 + particle.state.offset[1];
        }
        for position in &mut data.positions {
            *position = (Vec3::from_array(*position) + origin).to_array();
        }
    }
    Ok(data.map(Vertices::mesh))
}

fn geometry(
    state: &ParticleState,
    declaration: &Declaration,
    heading: f32,
    texture_size: [u32; 2],
    camera: Mat3,
) -> Result<Option<Vertices>> {
    ensure!(
        texture_size.iter().all(|&v| v > 0) && camera.is_finite() && heading.is_finite(),
        "invalid particle texture size or orientation"
    );
    if matches!(declaration, Declaration::ModelParticle { .. }) {
        return Ok(None);
    }
    let p = declaration.particle_visual()?;
    let gradient = declaration.template()?.gradient;
    let anchored = p.anchored;
    let mut out = Vertices::default();
    let colors = state.colors.map(|c| c.map(|v| f32::from(v as u8) / 255.));
    let uv = Uv::new(state.uv, texture_size);
    let dimensions = match state.geometry {
        ParticleGeometry::Size { value, .. } | ParticleGeometry::Spiral { value, .. } => {
            Vec3::from_array(value)
        }
        ParticleGeometry::BillboardTrail { size, radius, .. } => {
            Vec3::new(size[0], size[1], radius)
        }
        _ => Vec3::ZERO,
    };
    let offset = Vec3::from_array(state.orbit);
    let mut angles = Vec3::from_array(state.angles);
    let basis = orientation(p, angles, heading, camera);
    match &p.geometry {
        ParticleShape::Quad => {
            let points = quad(dimensions, offset);
            let anchor = Vec3::Y * if anchored { dimensions.x * 0.5 } else { 0. };
            for _ in 0..=p.copies {
                let basis = orientation(p, angles, heading, camera);
                out.strip(points.map(|v| basis * v + anchor), colors, uv);
                angles.z += f32::from(p.copy_rotation);
            }
        }
        ParticleShape::Ring {
            segments,
            flared,
            hidden,
            uv_layout,
        } => {
            if *hidden {
                return Ok(None);
            }
            let segments = *segments;
            for segment in 0..segments {
                out.strip(
                    ring(dimensions, anchored, *flared, segment, segments).map(|v| basis * v),
                    colors,
                    uv.panel(*uv_layout, u16::from(segment), state.geometry_count),
                );
            }
        }
        ParticleShape::Orbit => {
            let orbit = Mat3::from_rotation_y((angles.y + (90. + heading)) * DEGREES)
                * Mat3::from_rotation_x(angles.x * DEGREES);
            let orbit = if p.orientation == Orientation::Billboard {
                camera * orbit
            } else {
                orbit
            };
            let points = quad(dimensions, Vec3::ZERO);
            let anchor = Vec3::Y * if anchored { dimensions.x * 0.5 } else { 0. };
            let mut phase = angles.z;
            for segment in 0..state.geometry_count {
                let c = circle(phase);
                let center = orbit * Vec3::new(c.x * dimensions.z, c.y * dimensions.z, 0.);
                for copy in 0..=p.copies {
                    let spin = offset.z.mul_add(
                        f32::from(segment),
                        offset.x + f32::from(copy) * f32::from(p.copy_rotation),
                    );
                    let basis = camera * Mat3::from_rotation_z(spin * DEGREES);
                    out.strip(points.map(|v| basis * v + center + anchor), colors, uv);
                }
                phase += 360. / f32::from(state.geometry_count);
            }
        }
        ParticleShape::BillboardTrail => {
            let ParticleGeometry::BillboardTrail {
                segment_size_step,
                segment_offset,
                segment_angle_step,
                steps_per_segment,
                ..
            } = state.geometry
            else {
                bail!("billboard trail has incompatible particle state");
            };
            let orbit = if p.orientation == Orientation::Billboard {
                camera
            } else if p.orientation == Orientation::Camera {
                camera
                    * Mat3::from_rotation_y(angles.y * DEGREES)
                    * Mat3::from_rotation_x(angles.x * DEGREES)
            } else {
                Mat3::from_rotation_y((angles.y + heading) * DEGREES)
                    * Mat3::from_rotation_x(angles.x * DEGREES)
            };
            let step = Mat3::from_rotation_y(heading * DEGREES) * Vec3::from_array(segment_offset);
            let mut size = dimensions;
            let mut origin = Vec3::ZERO;
            let mut spin = offset.x;
            for segment in 0..state.geometry_count {
                let c = circle(angles.z);
                let center = orbit * Vec3::new(c.x * size.z, c.y * size.z, 0.) + origin;
                let anchor = Vec3::Y * if anchored { size.y } else { 0. };
                let color = segment_color(state, gradient, segment, state.geometry_count);
                for copy in 0..=p.copies {
                    let basis = camera
                        * Mat3::from_rotation_z(
                            (spin + f32::from(copy) * f32::from(p.copy_rotation)) * DEGREES,
                        );
                    out.strip(
                        quad(size, Vec3::ZERO).map(|v| basis * v + center + anchor),
                        [color; 2],
                        uv,
                    );
                }
                for _ in 0..steps_per_segment {
                    origin -= step;
                    angles.z -= segment_angle_step;
                    size += Vec3::from_array(segment_size_step);
                    spin -= offset.z;
                }
            }
        }
        ParticleShape::Spiral {
            radius_step,
            segment_offset,
            steps_per_segment,
            uv_layout,
        } => {
            let ParticleGeometry::Spiral {
                segment_angle_step, ..
            } = state.geometry
            else {
                bail!("spiral has incompatible particle state");
            };
            let basis = Mat3::from_rotation_y((angles.y + heading) * DEGREES)
                * Mat3::from_rotation_x(angles.x * DEGREES);
            let basis = if p.orientation == Orientation::Billboard {
                camera * basis
            } else {
                basis
            };
            let step = Mat3::from_rotation_y(heading * DEGREES) * Vec3::from_array(*segment_offset);
            let mut radius = dimensions.z;
            let mut phase = angles.z;
            let mut origin = Vec3::ZERO;
            let mut sample = |radius: f32, phase: f32| {
                let c = circle(phase);
                let pair = [
                    Vec3::new(
                        radius * c.x,
                        radius * c.y,
                        dimensions.x * if anchored { 1. } else { 0.5 },
                    ),
                    Vec3::new(
                        (radius + dimensions.y) * c.x,
                        (radius + dimensions.y) * c.y,
                        if anchored { 0. } else { -dimensions.x * 0.5 },
                    ),
                ]
                .map(|v| basis * v + origin);
                for _ in 0..*steps_per_segment {
                    origin -= step;
                }
                pair
            };
            let mut previous = sample(radius, phase);
            for segment in 0..state.geometry_count {
                for _ in 0..*steps_per_segment {
                    radius -= *radius_step;
                    phase -= segment_angle_step;
                }
                let next = sample(radius, phase);
                let color = segment_color(state, gradient, segment, state.geometry_count);
                out.strip(
                    [previous[0], next[0], previous[1], next[1]],
                    [color; 2],
                    uv.panel(*uv_layout, u16::from(segment), state.geometry_count),
                );
                previous = next;
            }
        }
        ParticleShape::Disc { segments } => {
            let count = *segments;
            let z = if anchored { -dimensions.z * 0.5 } else { 0. };
            for segment in 0..count {
                let point = |i| {
                    let c = circle(f32::from(i) * 360. / f32::from(count));
                    Vec3::new(c.x * dimensions.z, c.y * dimensions.z, z)
                };
                out.triangle(
                    [point(segment + 1), point(segment), Vec3::new(0., 0., z)].map(|v| basis * v),
                    [colors[1], colors[1], colors[0]],
                    [uv.at(0), uv.at(1), uv.at(2)],
                );
            }
        }
        ParticleShape::Shell {
            segments,
            elliptical,
            uv_layout,
        } => {
            let count = *segments;
            let basis = world_orientation(angles, heading);
            for segment in 0..count {
                out.strip(
                    shell(dimensions, anchored, *elliptical, segment, count).map(|v| basis * v),
                    colors,
                    uv.panel(*uv_layout, u16::from(segment), state.geometry_count),
                );
            }
        }
        ParticleShape::Sphere { columns } => {
            // Sphere tessellation is independent of animated segment counts and stays world-oriented.
            let basis = world_orientation(angles, heading);
            let columns = *columns;
            for row in 0..10 {
                let colors = [row, row + 1].map(|r| segment_color(state, gradient, r, 10));
                for column in 0..columns {
                    out.strip(
                        sphere(dimensions, row, column, columns).map(|v| basis * v),
                        colors,
                        uv,
                    );
                }
            }
        }
        ParticleShape::VertexQuad {
            copy_axis,
            local_copies,
        } => {
            let ParticleGeometry::Quad { vertices, .. } = state.geometry else {
                bail!("vertex particle has incompatible state");
            };
            for copy in 0..=p.copies {
                let (basis, shift) = if p.orientation == Orientation::Billboard {
                    (
                        camera * Mat3::from_rotation_z(angles.z * DEGREES),
                        camera * offset,
                    )
                } else {
                    (world_orientation(angles, heading), Vec3::ZERO)
                };
                let local = if *local_copies {
                    Mat3::from_rotation_z(f32::from(copy) * f32::from(p.copy_rotation) * DEGREES)
                } else {
                    angles[usize::from(*copy_axis)] += f32::from(p.copy_rotation);
                    Mat3::IDENTITY
                };
                out.strip(
                    vertices.map(|v| basis * local * Vec3::from_array(v) + shift),
                    colors,
                    uv,
                );
            }
        }
        ParticleShape::Unsupported { reason } => bail!("{reason}"),
    }
    ensure!(
        out.positions.iter().flatten().all(|v| v.is_finite()),
        "particle geometry overflow"
    );
    Ok(Some(out))
}

fn world_orientation(angles: Vec3, heading: f32) -> Mat3 {
    Mat3::from_rotation_y((angles.y + heading) * DEGREES)
        * Mat3::from_rotation_z(angles.z * DEGREES)
        * Mat3::from_rotation_x(angles.x * DEGREES)
}

fn orientation(p: &ParticleVisual, angles: Vec3, heading: f32, camera: Mat3) -> Mat3 {
    if p.orientation == Orientation::Billboard {
        camera * Mat3::from_rotation_z(angles.z * DEGREES)
    } else if p.orientation == Orientation::Camera {
        camera
            * Mat3::from_rotation_y(angles.y * DEGREES)
            * Mat3::from_rotation_x(angles.x * DEGREES)
            * Mat3::from_rotation_z(angles.z * DEGREES)
    } else {
        world_orientation(angles, heading)
    }
}

fn segment_color(state: &ParticleState, gradient: bool, segment: u8, count: u8) -> [f32; 4] {
    std::array::from_fn(|channel| {
        let [first, last] = state.colors.map(|c| i32::from(c[channel]));
        let step = if gradient {
            (first - last) / i32::from(count)
        } else {
            0
        };
        f32::from((first - step * i32::from(segment)) as u8) / 255.
    })
}
fn circle(degrees: f32) -> Vec2 {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Vec2::new(cos, sin)
}

fn quad(size: Vec3, offset: Vec3) -> [Vec3; 4] {
    [
        Vec3::new(-0.5, 0.5, 0.),
        Vec3::new(0.5, 0.5, 0.),
        Vec3::new(-0.5, -0.5, 0.),
        Vec3::new(0.5, -0.5, 0.),
    ]
    .map(|p| p * Vec3::new(size.y, size.x, 0.) + offset)
}

fn sphere(size: Vec3, row: u8, column: u8, columns: u8) -> [Vec3; 4] {
    std::array::from_fn(|i| {
        let latitude = circle((i16::from(row) - 5 + (i / 2) as i16) as f32 * 18.);
        let longitude = circle((f32::from(column) + (i & 1) as f32) * 360. / f32::from(columns));
        // Apply the radial scale to X and Y, and the depth scale to Z.
        Vec3::new(
            longitude.x * latitude.x * size.z,
            longitude.y * latitude.x * size.z,
            latitude.y * size.x,
        )
    })
}

fn ring(size: Vec3, anchored: bool, flared: bool, segment: u8, count: u8) -> [Vec3; 4] {
    let height = if flared { 0. } else { size.x };
    let bias = if anchored { 0. } else { height * 0.5 };
    std::array::from_fn(|i| {
        let pair = i / 2;
        let c = circle((f32::from(segment) + (i & 1) as f32) * 360. / f32::from(count));
        let radius_x = size.z + if pair == 0 { size.y } else { 0. };
        let radius_y = if flared {
            size.x + if pair == 0 { size.y } else { 0. }
        } else {
            radius_x
        };
        Vec3::new(
            c.x * radius_x,
            c.y * radius_y,
            if pair == 0 { height - bias } else { -bias },
        )
    })
}

fn shell(size: Vec3, anchored: bool, elliptical: bool, segment: u8, count: u8) -> [Vec3; 4] {
    let bias = if anchored { 0. } else { size.x * 0.5 };
    std::array::from_fn(|i| {
        let upper = i < 2;
        let c = circle(
            (f32::from(segment) + if elliptical { (i & 1) as f32 } else { 0. }) * 180.
                / f32::from(count),
        );
        if elliptical {
            Vec3::new(
                size.z * c.x,
                (size.z + if upper { size.y } else { 0. }) * c.y,
                if upper { size.x * c.y - bias } else { -bias },
            )
        } else {
            // This authored variant duplicates each pair, producing degenerate panels.
            let radius = size.z + if upper { size.y } else { 0. };
            Vec3::new(
                radius * c.x,
                radius * c.y,
                if upper { size.x - bias } else { -bias },
            )
        }
    })
}

#[derive(Clone, Copy)]
struct Uv {
    rect: [i32; 4],
    size: [u32; 2],
}
impl Uv {
    fn new(rect: [i16; 4], size: [u32; 2]) -> Self {
        Self {
            rect: rect.map(i32::from),
            size,
        }
    }
    fn at(self, vertex: usize) -> [f32; 2] {
        [
            (self.rect[0] + (vertex & 1) as i32 * self.rect[2]) as f32 / self.size[0] as f32,
            (self.rect[1] + (vertex / 2) as i32 * self.rect[3]) as f32 / self.size[1] as f32,
        ]
    }
    fn panel(mut self, layout: UvLayout, panel: u16, columns: u8) -> Self {
        let column = match layout {
            UvLayout::Repeat => 0,
            UvLayout::Advance => panel,
            UvLayout::Cycle if columns == 0 => panel,
            UvLayout::Cycle => panel % u16::from(columns),
            UvLayout::HalfWidthCycle => {
                self.rect[2] >>= 1;
                panel % (u16::from(columns.max(1)) * 2)
            }
            UvLayout::AlternatingHalves { panels_per_half } => {
                self.rect[2] >>= 1;
                (panel / u16::from(panels_per_half)) % 2
            }
        };
        self.rect[0] += self.rect[2] * i32::from(column);
        self
    }
}

#[derive(Default)]
pub(super) struct Vertices {
    pub(super) positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    pub(super) colors: Vec<[f32; 4]>,
    pub(super) uv: Vec<[f32; 2]>,
}
impl Vertices {
    fn strip(&mut self, points: [Vec3; 4], colors: [[f32; 4]; 2], uv: Uv) {
        for indices in [[0, 1, 2], [2, 1, 3]] {
            self.triangle(
                indices.map(|i| points[i]),
                indices.map(|i| colors[i / 2]),
                indices.map(|i| uv.at(i)),
            );
        }
    }
    fn triangle(&mut self, points: [Vec3; 3], colors: [[f32; 4]; 3], uv: [[f32; 2]; 3]) {
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or(Vec3::Z);
        self.positions.extend(points.map(|p| p.to_array()));
        self.normals.extend([normal.to_array(); 3]);
        self.colors.extend(colors);
        self.uv.extend(uv);
    }
    pub(super) fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.uv.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn fixture_particle(text: &str, field: &str, member: &str) -> Declaration {
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        serde_json::from_value(value[field][member].clone()).unwrap()
    }

    fn visual(declaration: &mut Declaration) -> &mut ParticleVisual {
        let Declaration::Particle { visual, .. } = declaration else {
            panic!("sprite fixture");
        };
        visual
    }

    #[test]
    fn sphere_geometry_is_world_oriented_and_does_not_advance_state() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let mut state = declaration.particle()?.state;
        state.angles = [0.; 3];
        let ParticleGeometry::Size { value, .. } = &mut state.geometry else {
            panic!("sphere dimensions");
        };
        *value = [10., 777., 20.];
        let before = state.clone();
        let camera = Mat3::from_rotation_x(0.7) * Mat3::from_rotation_y(1.1);
        let output = geometry(&state, &declaration, 0., [512; 2], camera)?.unwrap();
        let held = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY)?.unwrap();
        assert_eq!(output.positions.len(), 960);
        assert_eq!(output.positions, held.positions);
        assert_eq!(state, before);
        assert!(
            output.positions.iter().all(|point| point[0].abs() <= 20.
                && point[1].abs() <= 20.
                && point[2].abs() <= 10.)
        );
        visual(&mut declaration).geometry = ParticleShape::Sphere { columns: 8 };
        validate(&declaration)?;
        let coarse = geometry(&state, &declaration, 0., [512; 2], camera)?.unwrap();
        assert_eq!(coarse.positions.len(), output.positions.len() / 2);
        visual(&mut declaration).geometry = ParticleShape::Sphere { columns: 0 };
        assert!(validate(&declaration).is_err());
        Ok(())
    }

    #[test]
    fn spiral_vertices_follow_live_signed_segment_steps() -> Result<()> {
        let declaration = fixture_particle(
            include_str!("../../../game/tests/fixtures/sheena-normal-particles.json"),
            "declarations",
            "45",
        );
        validate(&declaration)?;
        let mut state = declaration.particle()?.state;
        state.angles = [0.; 3];
        let forward = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY)?.unwrap();
        let ParticleGeometry::Spiral {
            segment_angle_step, ..
        } = &mut state.geometry
        else {
            panic!("spiral dimensions");
        };
        *segment_angle_step = -*segment_angle_step;
        let reverse = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY)?.unwrap();
        assert_eq!(forward.positions.len(), 24);
        assert_eq!(reverse.positions.len(), 24);
        assert!(forward.positions.iter().any(|point| point[1] < -1.));
        for (forward, reverse) in forward.positions.iter().zip(&reverse.positions) {
            assert_eq!(forward[0], reverse[0]);
            assert_eq!(forward[1], -reverse[1]);
            assert_eq!(forward[2], reverse[2]);
        }
        Ok(())
    }

    #[test]
    fn orbit_billboards_keep_fractional_angles_and_radius() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let visual = visual(&mut declaration);
        visual.geometry = ParticleShape::Orbit;
        visual.orientation = Orientation::World;
        visual.anchored = false;
        let mut state = declaration.particle()?.state;
        state.geometry_count = 4;
        state.angles = [0., -90., 0.5];
        state.orbit = [0.; 3];
        let ParticleGeometry::Size { value, .. } = &mut state.geometry else {
            panic!("orbit size");
        };
        *value = [2., 2., 10.];
        let output = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY)?.unwrap();
        assert_eq!(output.positions.len(), 24);
        for quad in output.positions.chunks_exact(6) {
            let center = (Vec3::from_array(quad[0]) + Vec3::from_array(quad[5])) * 0.5;
            assert!((center.length() - 10.).abs() < 0.00001);
        }
        let first = (output.positions[0][1] + output.positions[5][1]) * 0.5;
        assert!(first > 0. && first < 0.1);
        Ok(())
    }

    #[test]
    fn shadow_batch_keeps_footprints_and_fades_at_edges() {
        assert!(shadows(std::iter::empty()).is_none());
        let mesh = shadows(
            [
                ([10., 1., 30.], 20., [0, 0, 0, 255]),
                ([90., 1., 30.], 10., [0, 0, 0, 128]),
            ]
            .into_iter(),
        )
        .unwrap();
        let Some(VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        assert!(points.iter().any(|p| p[0] < 10.));
        assert!(points.iter().any(|p| p[0] > 90.));
        assert!(points.iter().all(|p| p[1] == 1.
            && ((p[0] - 10.).hypot(p[2] - 30.) <= 20.001
                || (p[0] - 90.).hypot(p[2] - 30.) <= 10.001)));
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        assert!(normals.iter().all(|n| n[1] > 0.99));
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors");
        };
        assert!(colors.iter().any(|c| c[3] == 0.));
        assert!(colors.iter().any(|c| c[3] == 0.4));
        assert!(colors.iter().any(|c| c[3] > 0. && c[3] < 0.4));
        assert!(colors.iter().all(|c| c[3] >= 0. && c[3] <= 0.4));
    }

    #[test]
    fn uv_coordinates_do_not_wrap_or_remove_zero_size_geometry() {
        let uv = Uv::new([32764, 0, 8, -16], [512, 256]).panel(UvLayout::Advance, 1, 0);
        assert_eq!(uv.at(0), [32772. / 512., 0.]);
        assert_eq!(uv.at(3), [32780. / 512., -16. / 256.]);
        let mut vertices = Vertices::default();
        vertices.strip(
            quad(Vec3::new(80., 40., 0.), Vec3::ZERO),
            [[1.; 4]; 2],
            Uv::new([0; 4], [512; 2]),
        );
        assert_eq!(vertices.positions.len(), 6);
        assert_eq!(vertices.uv, [[0.; 2]; 6]);
        let uv = Uv::new([2, 0, 8, 16], [1; 2]);
        assert_eq!(uv.panel(UvLayout::Cycle, 128, 0).at(0), [1026., 0.]);
        assert_eq!(uv.panel(UvLayout::HalfWidthCycle, 1, 0).at(0), [6., 0.]);
        assert_eq!(uv.panel(UvLayout::HalfWidthCycle, 2, 0).at(0), [2., 0.]);
    }

    #[test]
    fn particle_uv_cycles_keep_columns_127_and_128_unsigned() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let mut state = declaration.particle()?.state;
        state.uv = [2, 3, 8, 16];
        for shape in [
            ParticleShape::Ring {
                segments: 130,
                flared: false,
                hidden: false,
                uv_layout: UvLayout::Cycle,
            },
            ParticleShape::Shell {
                segments: 130,
                elliptical: true,
                uv_layout: UvLayout::Cycle,
            },
        ] {
            visual(&mut declaration).geometry = shape;
            for columns in [127, 128] {
                state.geometry_count = columns;
                let output = geometry(&state, &declaration, 0., [1; 2], Mat3::IDENTITY)?.unwrap();
                let last = usize::from(columns - 1);
                assert_eq!(output.uv[last * 6], [2. + 8. * last as f32, 3.]);
                assert_eq!(output.uv[usize::from(columns) * 6], [2., 3.]);
                assert_eq!(output.uv[usize::from(columns + 1) * 6], [10., 3.]);
                let uv = Uv::new(state.uv, [1; 2]);
                let halves = u16::from(columns) * 2;
                assert_eq!(
                    uv.panel(UvLayout::HalfWidthCycle, halves - 1, columns)
                        .at(0),
                    [2. + 4. * f32::from(halves - 1), 3.]
                );
                assert_eq!(
                    uv.panel(UvLayout::HalfWidthCycle, halves, columns).at(0),
                    [2., 3.]
                );
            }
        }
        Ok(())
    }

    #[test]
    fn particle_uv_layout_is_independent_of_ring_and_shell_tessellation() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let mut state = declaration.particle()?.state;
        state.uv = [2, 3, 8, 16];
        state.geometry_count = 3;
        for (coarse, fine, starts) in [
            (
                ParticleShape::Ring {
                    segments: 16,
                    flared: false,
                    hidden: false,
                    uv_layout: UvLayout::HalfWidthCycle,
                },
                ParticleShape::Ring {
                    segments: 32,
                    flared: false,
                    hidden: false,
                    uv_layout: UvLayout::HalfWidthCycle,
                },
                [2., 6., 10., 14., 18., 22., 2., 6.],
            ),
            (
                ParticleShape::Shell {
                    segments: 8,
                    elliptical: true,
                    uv_layout: UvLayout::AlternatingHalves { panels_per_half: 4 },
                },
                ParticleShape::Shell {
                    segments: 16,
                    elliptical: true,
                    uv_layout: UvLayout::AlternatingHalves { panels_per_half: 4 },
                },
                [2., 2., 2., 2., 6., 6., 6., 6.],
            ),
        ] {
            visual(&mut declaration).geometry = coarse;
            let coarse = geometry(&state, &declaration, 0., [1; 2], Mat3::IDENTITY)?.unwrap();
            visual(&mut declaration).geometry = fine;
            let fine = geometry(&state, &declaration, 0., [1; 2], Mat3::IDENTITY)?.unwrap();
            assert_eq!(coarse.uv, fine.uv[..coarse.uv.len()]);
            for (panel, start) in fine.uv.chunks_exact(6).zip(starts) {
                assert_eq!(panel[0], [start, 3.]);
                assert_eq!(panel[1], [start + 4., 3.]);
            }
            let (ParticleShape::Ring { uv_layout, .. } | ParticleShape::Shell { uv_layout, .. }) =
                &mut visual(&mut declaration).geometry
            else {
                unreachable!()
            };
            *uv_layout = UvLayout::Repeat;
            let repeated = geometry(&state, &declaration, 0., [1; 2], Mat3::IDENTITY)?.unwrap();
            assert!(
                repeated
                    .uv
                    .chunks_exact(6)
                    .all(|panel| panel[0] == [2., 3.] && panel[1] == [10., 3.])
            );
        }
        let ParticleShape::Shell { uv_layout, .. } = &mut visual(&mut declaration).geometry else {
            unreachable!()
        };
        *uv_layout = UvLayout::AlternatingHalves { panels_per_half: 0 };
        assert!(validate(&declaration).is_err());
        Ok(())
    }
}
