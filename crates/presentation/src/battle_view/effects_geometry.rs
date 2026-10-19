//! Draw-only particle geometry. Motion, palette selection and lifetime come from
//! the authoritative frame; drawing never advances a particle or consumes RNG.
use anyhow::{Result, bail, ensure};
use bevy::{mesh::VertexAttributeValues, prelude::*};
use resonance_battle::{ParticleFrame, ParticleGeometry, ParticleState};
use resonance_content::battle_effect::{
    declaration::Declaration,
    visual::{Orientation, ParticleShape, ParticleVisual, UvLayout},
};

const DEGREES: f32 = std::f32::consts::PI / 180.;

const SHADOW_SEGMENTS: usize = 16;
const SPHERE_LATITUDE_BANDS: u8 = 10;

pub(super) fn wing_sparkles<'a>(
    vertices: &mut Vertices,
    sparkles: impl Iterator<Item = &'a super::super::wings::Sparkle>,
    camera: Mat3,
) {
    vertices.clear();
    let uv = Uv::new([193, 385, 62, 62], [512; 2]);
    for sparkle in sparkles {
        let rotation = camera * Mat3::from_rotation_z(sparkle.roll.to_radians());
        let points =
            quad(Vec3::splat(20.), Vec3::ZERO).map(|point| sparkle.position + rotation * point);
        vertices.strip(points, [[1., 1., 1., sparkle.alpha()]; 2], uv);
    }
}

// Spatial variation is fixed across draws and independent of gameplay RNG.
const RIBBON_NOISE: [i16; 256] = {
    let mut samples = [0; 256];
    let mut state = 12345_u32;
    let mut index = 0;
    while index < samples.len() {
        state = state.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12_d687);
        samples[index] = ((state as i32 >> 16) % 1024) as i16;
        index += 1;
    }
    samples
};

/// Batch ground footprints as soft discs, without a texture or a draw per shadow.
pub(super) fn shadows(
    vertices: &mut Vertices,
    shadows: impl Iterator<Item = ([f32; 3], f32, [u8; 4])>,
) {
    vertices.clear();
    for (position, radius, rgba) in shadows {
        let center = Vec3::from_array(position);
        let color = rgba.map(|v| f32::from(v) / 255.);
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
}

pub(super) fn validate(declaration: &Declaration) -> Result<()> {
    if let Declaration::ModelParticle { visual, .. } = declaration {
        ensure!(
            visual.blend <= 2,
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
        ParticleShape::Sphere { columns } | ParticleShape::Hemisphere { columns } => {
            ensure!(*columns > 0, "sphere has no columns")
        }
        ParticleShape::VertexQuad { copy_axis, .. } => {
            ensure!(*copy_axis < 3, "invalid particle copy axis")
        }
        ParticleShape::Ribbon { .. } => {
            ensure!(
                matches!(template.state.geometry, ParticleGeometry::Ribbon { jitter, .. } if jitter >= 0.)
                    && visual.copies == 0,
                "invalid ribbon geometry"
            );
        }
        ParticleShape::TriangleBand => {
            ensure!(
                matches!(template.state.geometry, ParticleGeometry::Size { .. })
                    && visual.copies == 0,
                "invalid triangle band geometry"
            );
        }
        ParticleShape::RadialQuads { .. } => {
            ensure!(
                template.state.geometry_count > 0 && visual.copies == 0,
                "invalid radial quad geometry"
            );
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
        ParticleShape::Orbit
            | ParticleShape::BillboardTrail
            | ParticleShape::Spiral { .. }
            | ParticleShape::Ribbon { .. }
    ) {
        ensure!(
            template.state.geometry_count > 0,
            "particle has no geometry segments"
        );
    }
    Ok(())
}

pub(super) fn particle(
    data: &mut Vertices,
    particle: &ParticleFrame,
    declaration: &Declaration,
    texture_size: [u32; 2],
    camera: Mat3,
    bone_world: Option<Mat4>,
) -> Result<()> {
    data.clear();
    ensure!(
        declaration.template()?.owner_bone.is_some() == bone_world.is_some(),
        "particle owner bone is not bound"
    );
    fill_geometry(
        data,
        &particle.state,
        declaration,
        if bone_world.is_some() {
            0.
        } else {
            particle.heading
        },
        texture_size,
        camera,
    )?;
    if !data.positions.is_empty() {
        let offset = Vec3::from_array(particle.state.offset);
        if let Some(matrix) = bone_world {
            let normal_matrix = Mat3::from_mat4(matrix).inverse().transpose();
            ensure!(
                matrix.is_finite() && normal_matrix.is_finite(),
                "invalid particle bone matrix"
            );
            for position in &mut data.positions {
                *position = matrix
                    .transform_point3(Vec3::from_array(*position) + offset)
                    .to_array();
            }
            for normal in &mut data.normals {
                *normal = (normal_matrix * Vec3::from_array(*normal))
                    .normalize_or(Vec3::Z)
                    .to_array();
            }
        } else {
            let mut origin = Vec3::from_array(particle.origin) + offset;
            // Ground-relative sprites ignore emitter height.
            if declaration.particle_visual()?.ground_relative {
                origin.y = 0.1 + offset.y;
            }
            for position in &mut data.positions {
                *position = (Vec3::from_array(*position) + origin).to_array();
            }
        }
    }
    Ok(())
}

pub(super) fn group(
    out: &mut Vertices,
    state: &ParticleState,
    heading: f32,
    declaration: &Declaration,
    texture_size: [u32; 2],
    camera: Mat3,
    mut points: impl Iterator<Item = Result<Vec3>>,
) -> Result<()> {
    out.clear();
    ensure!(
        matches!(declaration.particle_visual()?.geometry, ParticleShape::Quad),
        "effect group requires billboard geometry"
    );
    let Some(first) = points.next().transpose()? else {
        return Ok(());
    };
    ensure!(first.is_finite(), "invalid particle group position");
    fill_geometry(out, state, declaration, heading, texture_size, camera)?;
    let base_len = out.positions.len();
    let offset = Vec3::from_array(state.offset);
    for point in points {
        let point = point?;
        ensure!(point.is_finite(), "invalid particle group position");
        // Keep the first quad local until all copies have been appended.
        for index in 0..base_len {
            let position = Vec3::from_array(out.positions[index]) + point + offset;
            out.positions.push(position.to_array());
        }
        out.normals.extend_from_within(..base_len);
        out.colors.extend_from_within(..base_len);
        out.uv.extend_from_within(..base_len);
    }
    for position in &mut out.positions[..base_len] {
        *position = (Vec3::from_array(*position) + first + offset).to_array();
    }
    Ok(())
}

fn fill_geometry(
    out: &mut Vertices,
    state: &ParticleState,
    declaration: &Declaration,
    heading: f32,
    texture_size: [u32; 2],
    camera: Mat3,
) -> Result<()> {
    out.clear();
    ensure!(
        texture_size.iter().all(|&v| v > 0) && camera.is_finite() && heading.is_finite(),
        "invalid particle texture size or orientation"
    );
    if matches!(declaration, Declaration::ModelParticle { .. }) {
        return Ok(());
    }
    let p = declaration.particle_visual()?;
    let gradient = declaration.template()?.gradient;
    let anchored = p.anchored;
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
        ParticleShape::NoDraw => return Ok(()),
        ParticleShape::Quad => {
            let points = quad(dimensions, offset);
            let anchor = Vec3::Y * if anchored { dimensions.x * 0.5 } else { 0. };
            for _ in 0..=p.copies {
                let basis = orientation(p, angles, heading, camera);
                out.strip(points.map(|v| basis * v + anchor), colors, uv);
                angles.z += f32::from(p.copy_rotation);
            }
        }
        ParticleShape::TriangleBand => {
            let directions: [Vec2; 3] =
                std::array::from_fn(|corner| circle(-30. + 120. * corner as f32));
            let depth = dimensions.x * if anchored { 1. } else { 0.5 };
            let point = |direction: Vec2, radius: f32| basis * (direction * radius).extend(depth);
            for edge in 0..directions.len() {
                let start = directions[edge];
                let end = directions[(edge + 1) % directions.len()];
                out.strip(
                    [
                        point(start, dimensions.z),
                        point(end, dimensions.z),
                        point(start, dimensions.z + dimensions.y),
                        point(end, dimensions.z + dimensions.y),
                    ],
                    colors,
                    uv,
                );
            }
        }
        ParticleShape::RadialQuads { flat, advance_u } => {
            let count = state.geometry_count;
            ensure!(count > 0, "radial particle has no segments");
            let points = quad(dimensions, Vec3::ZERO).map(|point| {
                if *flat {
                    point
                } else {
                    Vec3::new(0., point.x, point.y)
                }
            });
            let anchor = Vec3::Y * if anchored { dimensions.x * 0.5 } else { 0. };
            for segment in 0..count {
                let orbit =
                    Mat3::from_rotation_z(360. * f32::from(segment) / f32::from(count) * DEGREES);
                let panel = if *advance_u {
                    uv.panel(UvLayout::Advance, u16::from(segment), count)
                } else {
                    uv
                };
                out.strip(
                    points.map(|point| basis * (orbit * (point + Vec3::X * dimensions.z)) + anchor),
                    colors,
                    panel,
                );
            }
        }
        ParticleShape::Ribbon { depth } => {
            let ParticleGeometry::Ribbon {
                length,
                width,
                jitter,
                phase,
                ..
            } = state.geometry
            else {
                bail!("ribbon particle has no ribbon dimensions");
            };
            let count = state.geometry_count;
            ensure!(count > 0 && jitter >= 0., "invalid ribbon dimensions");
            let half_width = width * 0.5;
            let span = ((jitter * 0.5) as i32).max(1);
            let mut edge = [
                Vec3::new(-half_width, length, 0.),
                Vec3::new(half_width, length, 0.),
            ];
            for segment in 0..count {
                let last = segment == count - 1;
                let sample = RIBBON_NOISE[usize::from(phase.wrapping_add(segment))];
                let x = if last {
                    0.
                } else {
                    (i32::from(sample) * 2 % span) as f32
                };
                let y = length * (1. - f32::from(segment + 1) / f32::from(count));
                let next = if *depth || state.ribbon_depth {
                    [Vec3::new(x, y, -half_width), Vec3::new(x, y, half_width)]
                } else {
                    [
                        Vec3::new(x - half_width, y, 0.),
                        Vec3::new(x + half_width, y, 0.),
                    ]
                };
                let mut panel = uv;
                panel.rect[1] += panel.rect[3] * (i32::from(segment > 0) + i32::from(last));
                out.strip(
                    [edge[0], edge[1], next[0], next[1]].map(|point| basis * point),
                    [segment, segment + 1].map(|at| segment_color(state, gradient, at, count)),
                    panel,
                );
                edge = next;
            }
        }
        ParticleShape::Ring {
            segments,
            flared,
            hidden,
            uv_layout,
        } => {
            if *hidden {
                return Ok(());
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
            let displacement = std::array::from_fn(|axis| {
                state.segment_offset[axis].unwrap_or(segment_offset[axis])
            });
            let step = Mat3::from_rotation_y(heading * DEGREES) * Vec3::from_array(displacement);
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
        ParticleShape::Sphere { columns } | ParticleShape::Hemisphere { columns } => {
            let hemisphere = matches!(p.geometry, ParticleShape::Hemisphere { .. });
            let (first_row, row_count, basis) = if hemisphere {
                (SPHERE_LATITUDE_BANDS / 2, SPHERE_LATITUDE_BANDS / 2, basis)
            } else {
                // Full spheres stay world-oriented; domes also support camera-facing poses.
                (0, SPHERE_LATITUDE_BANDS, world_orientation(angles, heading))
            };
            let columns = *columns;
            for row in 0..row_count {
                let colors = [row, row + 1].map(|r| segment_color(state, gradient, r, row_count));
                for column in 0..columns {
                    out.strip(
                        sphere(dimensions, first_row + row, column, columns).map(|v| basis * v),
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
                    (orientation(p, angles, heading, camera), Vec3::ZERO)
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
    Ok(())
}

fn world_orientation(angles: Vec3, heading: f32) -> Mat3 {
    Mat3::from_rotation_y((angles.y + heading) * DEGREES)
        * Mat3::from_rotation_z(angles.z * DEGREES)
        * Mat3::from_rotation_x(angles.x * DEGREES)
}

fn orientation(p: &ParticleVisual, angles: Vec3, heading: f32, camera: Mat3) -> Mat3 {
    if matches!(
        p.orientation,
        Orientation::FixedWorld | Orientation::FollowMotion
    ) {
        Mat3::from_rotation_y(angles.y * DEGREES)
            * Mat3::from_rotation_x(angles.x * DEGREES)
            * Mat3::from_rotation_z(angles.z * DEGREES)
    } else if p.orientation == Orientation::Billboard {
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
        let latitude = circle(
            (f32::from(row) - f32::from(SPHERE_LATITUDE_BANDS) * 0.5 + (i / 2) as f32)
                * (180. / f32::from(SPHERE_LATITUDE_BANDS)),
        );
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
    pub(super) fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.colors.clear();
        self.uv.clear();
    }

    fn swap_mesh(&mut self, mesh: &mut Mesh) -> Result<()> {
        for (attribute, vertices) in [
            (Mesh::ATTRIBUTE_POSITION, &mut self.positions),
            (Mesh::ATTRIBUTE_NORMAL, &mut self.normals),
        ] {
            let Some(VertexAttributeValues::Float32x3(values)) = mesh.attribute_mut(attribute)
            else {
                bail!("particle mesh is missing {}", attribute.name);
            };
            std::mem::swap(values, vertices);
        }
        let Some(VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        else {
            bail!("particle mesh is missing colors");
        };
        std::mem::swap(colors, &mut self.colors);
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
        else {
            bail!("particle mesh is missing UVs");
        };
        std::mem::swap(uv, &mut self.uv);
        Ok(())
    }
}

/// Stage geometry in reusable CPU buffers before publishing a nonempty mesh.
/// Hidden and failed draws retain their last valid GPU geometry.
pub(super) fn update_mesh(
    mesh: &mut Mesh,
    vertices: &mut Vertices,
    draw: impl FnOnce(&mut Vertices) -> Result<()>,
) -> Result<bool> {
    vertices.clear();
    draw(vertices)?;
    if vertices.positions.is_empty() {
        return Ok(false);
    }
    match mesh.attribute_mut(Mesh::ATTRIBUTE_UV_1) {
        Some(VertexAttributeValues::Float32x2(uv)) => {
            uv.clear();
            uv.extend_from_slice(&vertices.uv);
        }
        _ => bail!("particle mesh is missing secondary UVs"),
    }
    vertices.swap_mesh(mesh)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(
        state: &ParticleState,
        declaration: &Declaration,
        heading: f32,
        texture_size: [u32; 2],
        camera: Mat3,
    ) -> Result<Option<Vertices>> {
        let mut vertices = Vertices::default();
        fill_geometry(
            &mut vertices,
            state,
            declaration,
            heading,
            texture_size,
            camera,
        )?;
        Ok((!vertices.positions.is_empty()).then_some(vertices))
    }

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
    fn changing_particle_geometry_keeps_hidden_and_failed_draws_valid() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let state = declaration.particle()?.state;
        let mut mesh = super::super::warm_mesh();
        let mut vertices = Vertices::default();
        let mesh_storage = |mesh: &Mesh| {
            [
                Mesh::ATTRIBUTE_POSITION,
                Mesh::ATTRIBUTE_NORMAL,
                Mesh::ATTRIBUTE_COLOR,
                Mesh::ATTRIBUTE_UV_0,
                Mesh::ATTRIBUTE_UV_1,
            ]
            .map(|attribute| match mesh.attribute(attribute).unwrap() {
                VertexAttributeValues::Float32x2(v) => (v.as_ptr() as usize, v.capacity()),
                VertexAttributeValues::Float32x3(v) => (v.as_ptr() as usize, v.capacity()),
                VertexAttributeValues::Float32x4(v) => (v.as_ptr() as usize, v.capacity()),
                _ => panic!("particle vertex format"),
            })
        };
        for (shape, count) in [
            (ParticleShape::Sphere { columns: 16 }, 960),
            (ParticleShape::Quad, 6),
            (ParticleShape::NoDraw, 0),
            (ParticleShape::Sphere { columns: 16 }, 960),
        ] {
            visual(&mut declaration).geometry = shape;
            visual(&mut declaration).copies = 0;
            let previous = mesh.clone();
            let storage = mesh_storage(&mesh);
            let visible = update_mesh(&mut mesh, &mut vertices, |vertices| {
                fill_geometry(vertices, &state, &declaration, 0., [512; 2], Mat3::IDENTITY)
            })?;
            assert_eq!(visible, count > 0);
            assert!(
                mesh.count_vertices() > 0,
                "empty meshes must never be uploaded"
            );
            if !visible {
                assert_eq!(mesh_storage(&mesh), storage);
                for (attribute, values) in previous.attributes() {
                    assert_eq!(mesh.attribute(attribute.id), Some(values));
                }
                continue;
            }
            assert_eq!(mesh.count_vertices(), count);
            let fresh =
                geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY)?.unwrap_or_default();
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                Some(&VertexAttributeValues::Float32x3(fresh.positions))
            );
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                Some(&VertexAttributeValues::Float32x3(fresh.normals))
            );
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_COLOR),
                Some(&VertexAttributeValues::Float32x4(fresh.colors))
            );
            for attribute in [Mesh::ATTRIBUTE_UV_0, Mesh::ATTRIBUTE_UV_1] {
                assert_eq!(
                    mesh.attribute(attribute),
                    Some(&VertexAttributeValues::Float32x2(fresh.uv.clone()))
                );
            }
            assert_eq!(
                mesh.primitive_topology(),
                bevy::mesh::PrimitiveTopology::TriangleList
            );
            assert!(mesh.indices().is_none());
        }
        // Failed generation must not publish partial or empty geometry either.
        let previous = mesh.clone();
        let storage = mesh_storage(&mesh);
        assert!(
            update_mesh(&mut mesh, &mut vertices, |vertices| {
                vertices.positions.push([0.; 3]);
                bail!("failed particle geometry")
            })
            .is_err()
        );
        assert_eq!(storage, mesh_storage(&mesh));
        for (attribute, values) in previous.attributes() {
            assert_eq!(mesh.attribute(attribute.id), Some(values));
        }
        Ok(())
    }

    #[test]
    fn radial_quads_keep_ring_centers_and_advance_texture_panels() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let look = visual(&mut declaration);
        look.geometry = ParticleShape::RadialQuads {
            flat: true,
            advance_u: true,
        };
        look.orientation = Orientation::Billboard;
        look.anchored = false;
        look.copies = 0;
        let template = declaration.template_mut()?;
        template.state.angles = [0.; 3];
        template.state.geometry_count = 12;
        template.state.uv = [0, 0, 16, 16];
        template.state.geometry = ParticleGeometry::Size {
            value: [40., 40., 128.],
            velocity: [0.; 3],
            acceleration: [0.; 3],
        };
        let state = template.state.clone();
        validate(&declaration)?;
        let data = geometry(&state, &declaration, 0., [256; 2], Mat3::IDENTITY)?.unwrap();
        assert_eq!(data.positions.len(), 72);
        for segment in 0..12 {
            let vertices = &data.positions[segment * 6..segment * 6 + 6];
            let center = vertices.iter().map(|&v| Vec3::from_array(v)).sum::<Vec3>() / 6.;
            assert!((center.truncate().length() - 128.).abs() < 0.001);
            assert_eq!(center.z, 0.);
            assert_eq!(data.uv[segment * 6][0], segment as f32 / 16.);
        }
        Ok(())
    }

    #[test]
    fn triangle_band_closes_with_consistent_winding_and_repeats_its_atlas_panel() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let look = visual(&mut declaration);
        look.geometry = ParticleShape::TriangleBand;
        look.orientation = Orientation::World;
        look.anchored = false;
        look.copies = 0;
        let template = declaration.template_mut()?;
        template.state.angles = [0.; 3];
        template.state.uv = [384, 384, 64, 64];
        template.state.geometry = ParticleGeometry::Size {
            value: [8., 96., 304.],
            velocity: [0.; 3],
            acceleration: [0.; 3],
        };
        let state = template.state.clone();
        validate(&declaration)?;
        let data = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY)?.unwrap();
        assert_eq!(data.positions.len(), 18);
        for edge in 0..3 {
            let start = edge * 6;
            let next = (edge + 1) % 3 * 6;
            assert_eq!(data.positions[start + 1], data.positions[next]);
            assert_eq!(data.positions[start + 5], data.positions[next + 2]);
            assert_eq!(data.uv[start..start + 6], data.uv[..6]);
            for (vertex, radius) in [304., 304., 400., 400., 304., 400.].into_iter().enumerate() {
                let point = Vec3::from_array(data.positions[start + vertex]);
                assert!((point.truncate().length() - radius).abs() < 0.001);
                assert_eq!(point.z, 4.);
            }
        }
        assert_eq!(data.uv[0], [0.75, 0.75]);
        assert_eq!(data.uv[5], [0.875, 0.875]);
        for triangle in data.positions.chunks_exact(3) {
            let [a, b, c] = std::array::from_fn(|i| Vec3::from_array(triangle[i]));
            assert!((b - a).cross(c - a).z < 0.);
        }
        visual(&mut declaration).copies = 1;
        assert!(validate(&declaration).is_err());
        Ok(())
    }

    #[test]
    fn group_billboards_use_live_bone_positions_and_empty_groups_draw_nothing() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let visual = visual(&mut declaration);
        visual.geometry = ParticleShape::Quad;
        visual.orientation = Orientation::Billboard;
        visual.copies = 0;
        let template = declaration.template_mut()?;
        template.owner_bone = None;
        template.state.effect_group = Some(9);
        template.state.angles = [0.; 3];
        template.state.orbit = [0.; 3];
        template.state.offset = [0., 3., 0.];
        template.state.geometry = ParticleGeometry::Size {
            value: [2., 4., 0.],
            velocity: [0.; 3],
            acceleration: [0.; 3],
        };
        let state = template.state.clone();
        let mut data = Vertices::default();
        group(
            &mut data,
            &state,
            0.,
            &declaration,
            [64; 2],
            Mat3::IDENTITY,
            std::iter::empty(),
        )?;
        assert!(data.positions.is_empty());
        let points = [Vec3::new(10., 20., 30.), Vec3::new(40., 50., 60.)];
        group(
            &mut data,
            &state,
            0.,
            &declaration,
            [64; 2],
            Mat3::IDENTITY,
            points.into_iter().map(Ok),
        )?;
        let positions = &data.positions;
        assert_eq!(positions.len(), 12);
        for (first, second) in positions[..6].iter().zip(&positions[6..]) {
            assert_eq!(
                Vec3::from_array(*second) - Vec3::from_array(*first),
                Vec3::splat(30.)
            );
            assert!(first[0] < 20. && first[1] < 30. && first[2] < 40.);
        }
        Ok(())
    }

    #[test]
    fn ribbon_connects_segments_uses_three_atlas_rows_and_preserves_state() -> Result<()> {
        let mut declaration = fixture_particle(
            include_str!("../../../content/tests/fixtures/special-guard-particles.json"),
            "actors",
            "34",
        );
        let visual = visual(&mut declaration);
        visual.geometry = ParticleShape::Ribbon { depth: false };
        visual.orientation = Orientation::Billboard;
        visual.copies = 0;
        let template = declaration.template_mut()?;
        template.gradient = true;
        template.state.angles = [0.; 3];
        template.state.uv = [7, 11, 4, 6];
        template.state.colors = [[128; 4], [0; 4]];
        template.state.geometry_count = 4;
        template.state.geometry = ParticleGeometry::Ribbon {
            length: 8.,
            width: 4.,
            jitter: 104.,
            phase: 6,
            phase_period: 0,
        };
        validate(&declaration)?;
        let mut state = declaration.particle()?.state;
        let before = state.clone();
        let output = geometry(&state, &declaration, 0., [64; 2], Mat3::IDENTITY)?.unwrap();
        assert_eq!(output.positions.len(), 24);
        assert_eq!(output.positions[0], [-2., 8., 0.]);
        assert_eq!(output.positions[2], [2., 6., 0.]);
        assert_eq!(output.positions[14], [36., 2., 0.]);
        assert_eq!(output.positions[23], [2., 0., 0.]);
        for segment in 1..4 {
            assert_eq!(
                output.positions[segment * 6 - 3],
                output.positions[segment * 6]
            );
            assert_eq!(
                output.positions[segment * 6 - 1],
                output.positions[segment * 6 + 1]
            );
        }
        assert_eq!(
            [0, 1, 2, 3].map(|segment| output.uv[segment * 6][1]),
            [11., 17., 17., 23.].map(|v| v / 64.)
        );
        assert_eq!(output.colors[2], [96. / 255.; 4]);
        assert_eq!(state, before);
        assert_eq!(
            output.positions,
            geometry(&state, &declaration, 0., [64; 2], Mat3::IDENTITY)?
                .unwrap()
                .positions
        );
        if let ParticleGeometry::Ribbon { phase, .. } = &mut state.geometry {
            *phase = 255;
        }
        for count in [4, 5, 6] {
            state.geometry_count = count;
            let output = geometry(&state, &declaration, 0., [64; 2], Mat3::IDENTITY)?.unwrap();
            assert_eq!(output.positions.len(), usize::from(count) * 6);
        }
        Ok(())
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
        let mut data = Vertices::default();
        shadows(&mut data, std::iter::empty());
        assert!(data.positions.is_empty());
        shadows(
            &mut data,
            [
                ([10., 1., 30.], 20., [0, 0, 0, 255]),
                ([90., 1., 30.], 10., [0, 0, 0, 128]),
            ]
            .into_iter(),
        );
        let points = &data.positions;
        assert!(points.iter().any(|p| p[0] < 10.));
        assert!(points.iter().any(|p| p[0] > 90.));
        assert!(points.iter().all(|p| p[1] == 1.
            && ((p[0] - 10.).hypot(p[2] - 30.) <= 20.001
                || (p[0] - 90.).hypot(p[2] - 30.) <= 10.001)));
        let normals = &data.normals;
        assert!(normals.iter().all(|n| n[1] > 0.99));
        let colors = &data.colors;
        assert!(colors.iter().any(|c| c[3] == 0.));
        assert!(colors.iter().any(|c| c[3] == 1.));
        assert!(colors.iter().any(|c| c[3] == 128. / 255.));
        assert!(colors.iter().all(|c| (0. ..=1.).contains(&c[3])));
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
