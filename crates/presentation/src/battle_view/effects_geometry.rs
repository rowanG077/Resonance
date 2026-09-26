//! Draw-only particle geometry. Motion, palette selection and lifetime come from
//! the authoritative frame; drawing never advances a particle or consumes RNG.
use anyhow::{Result, bail, ensure};
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};
use resonance_battle::{ParticleFrame, ParticleGeometry, ParticleState};
use resonance_content::battle_effect::declaration::{Body, Declaration, GeometryOperands, Prefix};

const DEGREES: f32 = 0.017453292;
const CIRCLE_DEGREES: f32 = 0.017453289;

/// 7A90C draws 15 triangles with a fixed 160 alpha; the sampled position and
/// radius come from the core, and the authored color supplies RGB only.
///
/// 473C4 saves VTXFMT0 with GX_S16 texture coordinates and seven fractional
/// bits; 47264 restores that format immediately before the 7A90C fan. The
/// authored value 124 therefore becomes 124/128 in normalized shader units.
const SHADOW_V: f32 = 124. / 128.;

pub(super) fn shadow(
    position: [f32; 3],
    radius: f32,
    rgb: [u8; 4],
    circle: &[[f32; 2]; 15],
) -> Mesh {
    let mut vertices = Vertices::default();
    let center = Vec3::from_array(position);
    let color = [
        f32::from(rgb[0]) / 255.,
        f32::from(rgb[1]) / 255.,
        f32::from(rgb[2]) / 255.,
        160. / 255.,
    ];
    for segment in 0..15 {
        let point = |index: usize| {
            let direction = circle[index % 15];
            center + Vec3::new(radius * direction[0], 0., -radius * direction[1])
        };
        // 7A90C emits [next, current, center] in the retail GX display list.
        // Retail triangles are clockwise in model space; the geometry importer
        // reverses that order at the GX-to-Bevy boundary so back-face culling
        // continues to use Bevy's counter-clockwise front-face convention.
        // Keep the source points and their UVs paired while applying the same
        // boundary conversion to this procedural fan.
        vertices.triangle(
            [point(segment), point(segment + 1), center],
            [color; 3],
            [[0., SHADOW_V], [0., SHADOW_V], [0.; 2]],
        );
    }
    vertices.mesh()
}

pub(super) fn validate(declaration: &Declaration) -> Result<()> {
    let p = &declaration.prefix;
    ensure!(
        matches!(p.kind, 3 | 4 | 5 | 7 | 8 | 10 | 11 | 12 | 15),
        "particle geometry {} is not prepared for drawing",
        p.kind
    );
    ensure!(
        p.flags_or_shake_amplitude & (0xc0000000 | 0x10000 | 0x200) == 0,
        "particle requires an unprepared bone or point-history drawing binding"
    );
    if matches!(p.kind, 4 | 10 | 11 | 12) {
        ensure!(
            p.additional_copies == 0,
            "procedural particle requests unsupported copies"
        );
    }
    if p.kind == 15 {
        ensure!(
            p.copy_axis_or_phase_period < 3,
            "invalid particle copy rotation axis"
        );
    }
    if matches!(p.kind, 7 | 8 | 10) {
        ensure!(p.geometry_count != 0, "particle has no geometry segments");
    }
    if p.kind == 10 {
        ensure!(
            p.geometry_phase != 0,
            "spiral particle has no samples per segment"
        );
    }
    if p.kind != 3 && p.resource_slot == 10 {
        ensure!(
            p.flags_or_shake_amplitude & 0x202000 == 0
                && matches!(
                    declaration.body,
                    Body::Particle {
                        geometry: GeometryOperands::Parameters { .. },
                        ..
                    }
                ),
            "screen particle requires an unprepared owner-list or texture-offset binding"
        );
    }
    Ok(())
}

pub(super) fn mesh(
    particle: &ParticleFrame,
    declaration: &Declaration,
    texture_size: [u32; 2],
    camera: Mat3,
    sine: &[f32; 450],
) -> Result<Option<Mesh>> {
    let mut data = geometry(
        &particle.state,
        declaration,
        particle.heading,
        texture_size,
        camera,
        sine,
    )?;
    if let Some(data) = &mut data {
        let mut origin =
            Vec3::from_array(particle.origin) + Vec3::from_array(particle.state.offset);
        // Ground-relative sprites ignore the emitter's height (7A100, 7A498).
        if declaration.prefix.flags_or_shake_amplitude & 0x1000 != 0 {
            origin.y = 0.1 + particle.state.offset[1];
        }
        for position in &mut data.positions {
            *position = (Vec3::from_array(*position) + origin).to_array();
        }
    }
    Ok(data.map(Vertices::mesh))
}

/// 47C44 projects each emitted world vertex. Packet offsets retain only the
/// signed low bytes of the authored words; the vertical factor is original data.
pub(super) fn screen_uv(
    mesh: &mut Mesh,
    declaration: &Declaration,
    camera: resonance_battle::CameraPose,
) -> Result<()> {
    let Body::Particle {
        geometry:
            GeometryOperands::Parameters {
                texture_offset_words,
                ..
            },
        ..
    } = &declaration.body
    else {
        bail!("screen particle has no texture offsets");
    };
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        bail!("screen particle has no world vertices");
    };
    let uv: Vec<_> = positions
        .iter()
        .map(|&position| {
            screen_coordinates(
                resonance_battle::project_screen_point(camera, position),
                *texture_offset_words,
            )
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv);
    Ok(())
}

fn screen_coordinates(point: [f32; 2], offset: [i32; 2]) -> [f32; 2] {
    let factor = f32::from_bits(0x3f892492);
    [
        point[0] / 640. + f32::from(offset[0] as i8) / 640.,
        factor.mul_add(
            point[1] / 480.,
            factor * (f32::from(offset[1] as i8) / 480.),
        ),
    ]
}

fn geometry(
    state: &ParticleState,
    declaration: &Declaration,
    heading: f32,
    texture_size: [u32; 2],
    camera: Mat3,
    sine: &[f32; 450],
) -> Result<Option<Vertices>> {
    ensure!(
        texture_size.iter().all(|&v| v > 0) && camera.is_finite() && heading.is_finite(),
        "invalid particle texture size or orientation"
    );
    let p = &declaration.prefix;
    let flags = p.flags_or_shake_amplitude;
    let anchored = flags & 2 != 0;
    let mut out = Vertices::default();
    let colors = state.colors.map(|c| c.map(|v| f32::from(v as u8) / 255.));
    let mut uv = Uv::new(state.uv, texture_size);
    let dimensions = match state.geometry {
        ParticleGeometry::Size { value, .. } => Vec3::from_array(value),
        ParticleGeometry::BillboardTrail { size, radius, .. } => {
            Vec3::new(size[0], size[1], radius)
        }
        _ => Vec3::ZERO,
    };
    let offset = Vec3::from_array(state.orbit);
    let mut angles = Vec3::from_array(state.angles);
    let basis = orientation(p, angles, heading, camera);
    match p.kind {
        3 => return Ok(None), // The scene renderer owns prepared model instances.
        5 => {
            let points = quad(dimensions, offset);
            let anchor = Vec3::Y * if anchored { dimensions.x * 0.5 } else { 0. };
            for _ in 0..=p.additional_copies {
                let basis = orientation(p, angles, heading, camera);
                out.strip(points.map(|v| basis * v + anchor), colors, uv);
                angles.z += f32::from(p.copy_rotation);
            }
        }
        4 => {
            // The native two-vertex ring sets the disabled packet bit.
            if flags & 0x20 != 0 {
                return Ok(None);
            }
            let segments = if flags & 0x100000 != 0 {
                32
            } else if flags & 0x1000000 != 0 {
                8
            } else {
                16
            };
            // 7A498 reads the signed byte at object+0x3a: geometry_count.
            let mut columns = state.geometry_count as i8;
            if segments == 32 {
                uv.rect[2] >>= 1;
                columns = if columns == 0 {
                    2
                } else {
                    columns.wrapping_mul(2)
                };
            }
            let mut phase = 0;
            for segment in 0..segments {
                out.strip(
                    ring(dimensions, anchored, flags & 0x800 != 0, segment, segments)
                        .map(|v| basis * v),
                    colors,
                    uv,
                );
                if flags & 0x10 != 0 || segments == 32 {
                    uv.cycle(&mut phase, columns);
                }
            }
        }
        7 => {
            let orbit = Mat3::from_rotation_y((angles.y + (90. + heading)) * DEGREES)
                * Mat3::from_rotation_x(angles.x * DEGREES);
            let orbit = if flags & 0x40 != 0 {
                camera * orbit
            } else {
                orbit
            };
            let points = quad(dimensions, Vec3::ZERO);
            let anchor = Vec3::Y * if anchored { dimensions.x * 0.5 } else { 0. };
            let mut phase = (angles.z as i16).rem_euclid(360) as f32;
            for segment in 0..state.geometry_count {
                // 34C binds cosine at sine + 90, not at a wrapped index.
                // 792CC quantizes the phase to signed degrees before lookup.
                let index = phase as usize;
                let c = Vec2::new(sine[90 + index], sine[index]);
                let center = orbit * Vec3::new(c.x * dimensions.z, c.y * dimensions.z, 0.);
                for copy in 0..=p.additional_copies {
                    let spin = offset.z.mul_add(
                        f32::from(segment),
                        offset.x + f32::from(copy) * f32::from(p.copy_rotation),
                    );
                    let basis = camera * Mat3::from_rotation_z(spin * DEGREES);
                    out.strip(points.map(|v| basis * v + center + anchor), colors, uv);
                }
                phase += 360. / f32::from(state.geometry_count);
                if phase >= 360. {
                    phase -= 360.;
                }
            }
        }
        8 => {
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
            let orbit = if flags & 0x40 != 0 {
                camera
            } else if flags & 0x2000000 != 0 {
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
                let color = segment_color(state, p, segment, state.geometry_count);
                for copy in 0..=p.additional_copies {
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
        10 => {
            let Body::Particle {
                geometry: GeometryOperands::Parameters { radius_step, .. },
                ..
            } = &declaration.body
            else {
                bail!("spiral has incompatible declaration");
            };
            let basis = Mat3::from_rotation_y((angles.y + heading) * DEGREES)
                * Mat3::from_rotation_x(angles.x * DEGREES);
            let basis = if flags & 0x40 != 0 {
                camera * basis
            } else {
                basis
            };
            let step = Mat3::from_rotation_y(heading * DEGREES)
                * Vec3::from_array(
                    p.acceleration_change_or_segment_offset
                        .map(|v| f32::from_bits(v.bits())),
                );
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
                for _ in 0..p.geometry_phase {
                    origin -= step;
                }
                pair
            };
            let mut previous = sample(radius, phase);
            for segment in 0..state.geometry_count {
                for _ in 0..p.geometry_phase {
                    radius -= f32::from_bits(radius_step.bits());
                    phase -= f32::from_bits(p.angle_step.bits());
                }
                let next = sample(radius, phase);
                let color = segment_color(state, p, segment, state.geometry_count);
                out.strip([previous[0], next[0], previous[1], next[1]], [color; 2], uv);
                previous = next;
                if flags & 0x10 != 0 {
                    uv.advance();
                }
            }
        }
        11 => {
            let count: u8 = if flags & 0x1000000 != 0 { 8 } else { 16 };
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
        12 => {
            let count = if flags & 0x100000 != 0 { 16 } else { 8 };
            let basis = world_orientation(angles, heading);
            let mut phase = 0;
            let start = uv.rect[0];
            if count == 16 {
                uv.rect[2] >>= 1;
            }
            for segment in 0..count {
                if count == 16 {
                    uv.rect[0] =
                        start.wrapping_add(uv.rect[2].wrapping_mul(i16::from((segment / 4) & 1)));
                }
                out.strip(
                    shell(dimensions, anchored, flags & 0x20 != 0, segment, count)
                        .map(|v| basis * v),
                    colors,
                    uv,
                );
                if flags & 0x10 != 0 && count != 16 {
                    uv.cycle(&mut phase, state.geometry_count as i8);
                }
            }
        }
        15 => {
            let ParticleGeometry::Quad { vertices, .. } = state.geometry else {
                bail!("vertex particle has incompatible state");
            };
            for copy in 0..=p.additional_copies {
                let (basis, shift) = if flags & 0x40 != 0 {
                    (
                        camera * Mat3::from_rotation_z(angles.z * DEGREES),
                        camera * offset,
                    )
                } else {
                    (world_orientation(angles, heading), Vec3::ZERO)
                };
                let local = if flags & 0x20 != 0 {
                    Mat3::from_rotation_z(f32::from(copy) * f32::from(p.copy_rotation) * DEGREES)
                } else {
                    angles[usize::from(p.copy_axis_or_phase_period)] += f32::from(p.copy_rotation);
                    Mat3::IDENTITY
                };
                out.strip(
                    vertices.map(|v| basis * local * Vec3::from_array(v) + shift),
                    colors,
                    uv,
                );
            }
        }
        kind => bail!("particle geometry {kind} was not prepared"),
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

fn orientation(p: &Prefix, angles: Vec3, heading: f32, camera: Mat3) -> Mat3 {
    if p.flags_or_shake_amplitude & 0x40 != 0 {
        camera * Mat3::from_rotation_z(angles.z * DEGREES)
    } else if p.flags_or_shake_amplitude & 0x2000000 != 0 {
        camera
            * Mat3::from_rotation_y(angles.y * DEGREES)
            * Mat3::from_rotation_x(angles.x * DEGREES)
            * Mat3::from_rotation_z(angles.z * DEGREES)
    } else {
        world_orientation(angles, heading)
    }
}

fn segment_color(state: &ParticleState, p: &Prefix, segment: u8, count: u8) -> [f32; 4] {
    std::array::from_fn(|channel| {
        let [first, last] = state.colors.map(|c| i32::from(c[channel]));
        let step = if p.flags_or_shake_amplitude & 8 != 0 {
            (first - last) / i32::from(count)
        } else {
            0
        };
        f32::from((first - step * i32::from(segment)) as u8) / 255.
    })
}
fn circle(degrees: f32) -> Vec2 {
    // Native shape initialization multiplies in single precision, calls the
    // double-precision SDK trigonometric functions, then rounds each result.
    let (sin, cos) = f64::from(degrees * CIRCLE_DEGREES).sin_cos();
    Vec2::new(cos as f32, sin as f32)
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
    rect: [i16; 4],
    size: [u32; 2],
}
impl Uv {
    fn new(rect: [i16; 4], size: [u32; 2]) -> Self {
        Self { rect, size }
    }
    fn at(self, vertex: usize) -> [f32; 2] {
        [
            (i32::from(self.rect[0]) + (vertex & 1) as i32 * i32::from(self.rect[2])) as f32
                / self.size[0] as f32,
            (i32::from(self.rect[1]) + (vertex / 2) as i32 * i32::from(self.rect[3])) as f32
                / self.size[1] as f32,
        ]
    }
    fn advance(&mut self) {
        self.rect[0] = self.rect[0].wrapping_add(self.rect[2]);
    }
    fn cycle(&mut self, phase: &mut i8, columns: i8) {
        self.advance();
        *phase = phase.wrapping_add(1);
        if *phase >= columns {
            *phase = 0;
            self.rect[0] = self.rect[0].wrapping_sub(self.rect[2].wrapping_mul(i16::from(columns)));
        }
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

    #[test]
    fn screen_coordinates_keep_native_viewport_offsets_and_fused_vertical_step() {
        // Instruction-derived 47C44 cases: fdivs/fadds for U, fdivs/fmadds
        // for V, with rodata2598=0x3f892492. These are not GPU capture fixtures.
        assert_eq!(screen_coordinates([0., 0.], [0, 0]), [0., 0.]);
        assert_eq!(
            screen_coordinates([640., 448.], [0, 0]).map(f32::to_bits),
            [0x3f800000, 0x3f800000]
        );
        assert_eq!(
            screen_coordinates([320., 224.], [4, 4]).map(f32::to_bits),
            [0x3f01999a, 0x3f024924]
        );
        assert_eq!(
            screen_coordinates([320., 224.], [260, -252]),
            screen_coordinates([320., 224.], [4, 4])
        );
        assert_eq!(
            screen_coordinates([0., 0.], [127, 128]).map(f32::to_bits),
            [0x3e4b3333, 0xbe924925]
        );
        assert_eq!(
            screen_coordinates([0., 2.], [0, 4])[1].to_bits(),
            0x3c5b6db7
        );
    }

    fn stun() -> (Declaration, ParticleState) {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../battle/tests/fixtures/stun-particle-source.json"
        ))
        .unwrap();
        let declaration: Declaration =
            serde_json::from_value(source["declaration"].clone()).unwrap();
        let uv: Vec<resonance_content::battle_effect::UvRecord> =
            serde_json::from_value(source["uv"].clone()).unwrap();
        let state = declaration.particle(&uv).unwrap().state;
        (declaration, state)
    }

    #[test]
    fn orbit_billboards_use_original_extended_samples_and_truncated_phase() -> Result<()> {
        let (declaration, mut state) = stun();
        validate(&declaration)?;
        // Neutral orbit orientation isolates original 792CC's phase sampling.
        // -2.5 truncates to -2, wraps to 358, then advances by 90 degrees.
        state.angles = [0., -90., -2.5];
        let mut sine = [0.; 450];
        // Original BTLusual member5 words, including the unwrapped cosine448.
        for (index, bits) in [
            (88, 0x3f7f_d814),
            (178, 0x3d0e_f36d),
            (268, 0xbf7f_d813),
            (358, 0xbd0e_f415),
            (448, 0x3f7f_d813),
        ] {
            sine[index] = f32::from_bits(bits);
        }
        let before = state.clone();
        let output = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY, &sine)?.unwrap();
        assert_eq!(output.positions.len(), 24);
        let centers = [
            [0x4247_e0cf, 0xbfdf_5d61],
            [0x3fdf_5c5a, 0x4247_e0d0],
            [0xc247_e0cf, 0x3fdf_5c5a],
            [0xbfdf_5d61, 0xc247_e0cf],
        ];
        for (row, center) in centers.into_iter().enumerate() {
            let [x, y] = center.map(f32::from_bits);
            assert_eq!(output.positions[row * 6], [x - 16., y + 16., 0.]);
            assert_eq!(output.positions[row * 6 + 5], [x + 16., y - 16., 0.]);
        }
        let held = geometry(&state, &declaration, 0., [512; 2], Mat3::IDENTITY, &sine)?.unwrap();
        assert_eq!(held.positions, output.positions);
        assert_eq!(state, before);
        // A modulo cosine lookup would substitute the adjacent float at88.
        assert_ne!(sine[448].to_bits(), sine[88].to_bits());
        Ok(())
    }

    #[test]
    fn orbit_billboards_preserve_camera_facing_copies_and_world_height_anchor() -> Result<()> {
        let (mut declaration, mut state) = stun();
        declaration.prefix.flags_or_shake_amplitude |= 2 | 0x40;
        declaration.prefix.additional_copies = 1;
        declaration.prefix.copy_rotation = 90;
        declaration.prefix.geometry_count = 1;
        state.geometry_count = 1;
        state.angles = [0., -90., 0.];
        state.colors = [[128, 64, 32, 255], [64, 32, 16, 128]];
        let mut sine = [0.; 450];
        sine[90] = 1.;
        let camera = Mat3::from_cols(Vec3::Z, Vec3::Y, Vec3::NEG_X);
        let output = geometry(&state, &declaration, 0., [512; 2], camera, &sine)?.unwrap();
        assert_eq!(output.positions.len(), 12);
        assert_eq!(output.positions[0], [0., 32., 34.]);
        assert!(Vec3::from_array(output.positions[6]).abs_diff_eq(Vec3::new(0., 0., 34.), 0.00001));
        assert_eq!(output.colors[0], [128. / 255., 64. / 255., 32. / 255., 1.]);
        assert_eq!(
            output.colors[2],
            [64. / 255., 32. / 255., 16. / 255., 128. / 255.]
        );
        assert_eq!(output.uv[0], [1. / 512., 65. / 512.]);
        assert_eq!(output.uv[5], [31. / 512., 95. / 512.]);
        declaration.prefix.flags_or_shake_amplitude &= !0x40;
        let world_orbit = geometry(&state, &declaration, 0., [512; 2], camera, &sine)?.unwrap();
        assert_eq!(world_orbit.positions[0], [50., 32., -16.]);
        Ok(())
    }

    #[test]
    fn sampled_shadow_converts_to_upward_ccw_winding() {
        let circle = [
            [1., 0.],
            [0., 1.],
            [-1., 0.],
            [0., -1.],
            [1., 1.],
            [2., 1.],
            [2., 2.],
            [1., 2.],
            [-1., 2.],
            [-2., 2.],
            [-2., 1.],
            [-2., -1.],
            [-1., -2.],
            [1., -2.],
            [2., -2.],
        ];
        let mesh = shadow([10., 1.1, 30.], 20., [16, 32, 64, 255], &circle);
        let Some(VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors");
        };
        assert_eq!(points.len(), 45);
        assert!(points.iter().all(|p| p[1] == 1.1));
        assert_eq!(points[0], [30., 1.1, 30.]);
        assert_eq!(points[1], [10., 1.1, 10.]);
        assert_eq!(points[2], [10., 1.1, 30.]);
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        assert_eq!(normals[0], [0., 1., 0.]);
        assert!(
            colors
                .iter()
                .all(|c| *c == [16. / 255., 32. / 255., 64. / 255., 160. / 255.])
        );
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("UV0");
        };
        assert_eq!(uv[0], [0., SHADOW_V]);
        assert_eq!(uv[1], [0., SHADOW_V]);
        assert_eq!(uv[2], [0., 0.]);
    }

    #[test]
    fn uv_wraps_in_signed_pixels_without_removing_zero_size_geometry() {
        let mut uv = Uv::new([32764, 0, 8, -16], [512, 256]);
        uv.advance();
        assert_eq!(uv.at(0), [-32764. / 512., 0.]);
        assert_eq!(uv.at(3), [-32756. / 512., -16. / 256.]);
        let mut vertices = Vertices::default();
        vertices.strip(
            quad(Vec3::new(80., 40., 0.), Vec3::ZERO),
            [[1.; 4]; 2],
            Uv::new([0; 4], [512; 2]),
        );
        assert_eq!(vertices.positions.len(), 6);
        assert_eq!(vertices.uv, [[0.; 2]; 6]);
    }
}
