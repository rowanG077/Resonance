//! Terrain queries preserve source face order and the ship's water-only overlap rule.
use super::{Position, TileCoordinate};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::CollisionGroup,
    overworld::{CollisionTables, TILE_SIZE},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ground,
    AlternateGround,
    Ship,
    RestrictedGround,
}

#[derive(Debug, Clone, Copy)]
struct Face {
    points: [[f32; 3]; 3],
    surface: u8,
}
impl Face {
    fn contains(self, point: [f32; 3]) -> bool {
        // Original queries accept only upward winding and include shared edges.
        let [a, b, c] = self.points;
        if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) <= 0. {
            return false;
        }
        (0..3).all(|i| {
            let a = self.points[i];
            let b = self.points[(i + 1) % 3];
            (b[0] - a[0]) * (point[1] - a[1]) >= (b[1] - a[1]) * (point[0] - a[0])
        })
    }
    fn normal(self) -> [f32; 3] {
        let [a, b, c] = self.points.map(|p| p.map(f64::from));
        let u: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let v: [f64; 3] = std::array::from_fn(|i| c[i] - a[i]);
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
        normal.map(|v| (v / length) as f32)
    }
    fn correction(self, point: [f32; 3]) -> f32 {
        let n = self.normal().map(f64::from);
        let distance = (0..3)
            .map(|i| n[i] * (f64::from(self.points[0][i]) - f64::from(point[i])))
            .sum::<f64>();
        (n[2] * distance / n.iter().map(|v| v * v).sum::<f64>()) as f32
    }
    fn ground(self, point: [f32; 3], tables: &CollisionTables, mode: Mode) -> Surface {
        let normal = self.normal();
        let height = self.points[0][2]
            - (normal[0] * (point[0] - self.points[0][0])
                + normal[1] * (point[1] - self.points[0][1]))
                / normal[2];
        Surface {
            surface: self.surface,
            response: tables.surface_responses[usize::from(self.surface)][mode as usize],
            height,
            normal,
        }
    }
}

/// A prepared terrain tile. No file or resource discovery occurs during queries.
pub struct Mesh {
    faces: Vec<Face>,
}
impl Mesh {
    pub fn new(groups: &[CollisionGroup]) -> Result<Self> {
        let mut faces = Vec::new();
        for group in groups {
            group.validate()?;
            ensure!(group.surface < 16, "invalid world terrain surface");
            for triangle in &group.triangles {
                let points = triangle.map(|i| group.vertices[usize::from(i)]);
                let [a, b, c] = points;
                // The broad phase excludes faces whose horizontal vertices all
                // coincide. Retain other faces, including downward faces, since
                // they still occupy slots in the source's 2048-face budget.
                if a[..2] != b[..2] || b[..2] != c[..2] {
                    faces.push(Face {
                        points,
                        surface: group.surface as u8,
                    });
                }
            }
        }
        Ok(Self { faces })
    }
}

pub struct Terrain {
    tiles: BTreeMap<TileCoordinate, Mesh>,
    tables: CollisionTables,
}
impl Terrain {
    pub fn new(
        tiles: impl IntoIterator<Item = (TileCoordinate, Mesh)>,
        tables: CollisionTables,
    ) -> Result<Self> {
        tables.validate()?;
        let mut prepared = BTreeMap::new();
        for (coordinate, mesh) in tiles {
            ensure!(
                prepared.insert(coordinate, mesh).is_none(),
                "duplicate world terrain tile"
            );
        }
        ensure!(!prepared.is_empty(), "world has no terrain tiles");
        Ok(Self {
            tiles: prepared,
            tables,
        })
    }

    /// Collect the same center, south, north, west/east and diagonal neighborhoods
    /// as the native collision service. The caller supplies its search radius.
    pub fn query(&self, origin: Position, radius: f32) -> Result<Query<'_>> {
        self.collect(origin, radius, false)
    }

    /// Camera clearance uses vertex boxes, rather than the locomotion query's
    /// face midpoint. Tall walls can affect the view without being walkable.
    pub(super) fn camera_query(&self, origin: Position, radius: f32) -> Result<Query<'_>> {
        self.collect(origin, radius, true)
    }

    fn collect(&self, origin: Position, radius: f32, camera: bool) -> Result<Query<'_>> {
        ensure!(
            radius.is_finite() && radius > 0. && radius < TILE_SIZE / 2.,
            "invalid world terrain query radius"
        );
        let p = origin.local();
        let south = p[1] - radius < -TILE_SIZE / 2.;
        let north = p[1] + radius >= TILE_SIZE / 2.;
        let mut offsets = vec![[0, 0]];
        if south {
            offsets.push([0, 1]);
        }
        if north {
            offsets.push([0, -1]);
        }
        let horizontal = if p[0] - radius < -TILE_SIZE / 2. {
            -1
        } else if p[0] + radius >= TILE_SIZE / 2. {
            1
        } else {
            0
        };
        if horizontal != 0 {
            offsets.push([horizontal, 0]);
            if south {
                offsets.push([horizontal, 1]);
            } else if north {
                offsets.push([horizontal, -1]);
            }
        }
        let tile = origin.tile();
        let mut faces = Vec::new();
        for offset in offsets {
            let mesh = self
                .tiles
                .get(&tile.neighbor(offset))
                .context("world collision neighbor is not prepared")?;
            let translation = [
                f32::from(offset[0]) * TILE_SIZE,
                -f32::from(offset[1]) * TILE_SIZE,
            ];
            let local = [p[0] - translation[0], p[1] - translation[1]];
            for face in &mesh.faces {
                let [a, b, c] = face.points;
                // The native broad phase uses the midpoint between C and AB's
                // midpoint, not the centroid. Keep that selection and face order.
                let middle: [f32; 2] =
                    std::array::from_fn(|i| ((b[i] - a[i]) * 0.5 + a[i] - c[i]) * 0.5 + c[i]);
                let selected = if camera {
                    face.points.iter().any(|v| {
                        (v[0] - local[0]).abs() <= radius && (v[1] - local[1]).abs() <= radius
                    })
                } else {
                    (middle[0] - local[0]).powi(2) + (middle[1] - local[1]).powi(2)
                        <= radius * radius
                };
                if !selected {
                    continue;
                }
                if faces.len() == 2048 {
                    break;
                }
                faces.push(Face {
                    points: face
                        .points
                        .map(|v| [v[0] + translation[0], v[1] + translation[1], v[2]]),
                    surface: face.surface,
                });
            }
        }
        Ok(Query {
            faces,
            tables: &self.tables,
            origin: p,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub surface: u8,
    pub response: i8,
    pub height: f32,
    pub normal: [f32; 3],
}

pub struct Query<'a> {
    faces: Vec<Face>,
    tables: &'a CollisionTables,
    origin: [f32; 3],
}
impl Query<'_> {
    /// Maximum view pitch around a point displaced from this query's center.
    /// No terrain response or winding filter applies to camera obstructions.
    pub(super) fn camera_angle_bound(&self, displacement: [f32; 3]) -> f32 {
        let origin: [f32; 3] = std::array::from_fn(|i| self.origin[i] + displacement[i]);
        self.faces
            .iter()
            .flat_map(|face| face.points)
            .fold(170f32.to_radians(), |bound, point| {
                if point[2] <= origin[2] {
                    return bound;
                }
                let distance = (point[0] - origin[0]).hypot(point[1] - origin[1]);
                bound.min(std::f32::consts::PI - (point[2] - origin[2]).atan2(distance))
            })
    }
    fn face(&self, point: [f32; 3], mode: Mode) -> Option<Face> {
        let mut water = None;
        for face in &self.faces {
            if mode == Mode::Ship {
                if face.contains(point) {
                    // Any overlapping land invalidates water, regardless of order.
                    if face.surface != 11 {
                        return None;
                    }
                    water = Some(*face);
                }
            } else if self.tables.surface_responses[usize::from(face.surface)][mode as usize] != -1
                && face.contains(point)
            {
                return Some(*face);
            }
        }
        water
    }
    fn clear_at(&self, point: [f32; 3], mode: Mode) -> bool {
        let half = if mode == Mode::Ship {
            self.tables.mode2_probe_half_extent
        } else {
            self.tables.other_probe_half_extent
        };
        [[half, half], [-half, half], [half, -half], [-half, -half]]
            .into_iter()
            .all(|[x, z]| {
                self.face([point[0] + x, point[1] + z, point[2]], mode)
                    .is_some()
            })
    }
    pub fn surface(&self, mode: Mode) -> Option<Surface> {
        self.face(self.origin, mode)
            .map(|f| f.ground(self.origin, self.tables, mode))
    }
    pub fn has_clearance(&self, mode: Mode) -> bool {
        self.clear_at(self.origin, mode)
    }

    /// One native movement update: proposed heading, then +60°, then -60°.
    /// A rejected move still resolves vertical correction from containing faces.
    pub fn motion(&self, distance: f32, heading: f32, mode: Mode) -> Result<Motion> {
        ensure!(
            distance.is_finite() && distance >= 0. && heading.is_finite(),
            "invalid world motion"
        );
        let heading = heading.rem_euclid(std::f32::consts::TAU);
        for angle in [
            heading,
            heading + std::f32::consts::FRAC_PI_3,
            heading - std::f32::consts::FRAC_PI_3,
        ] {
            let angle = angle.rem_euclid(std::f32::consts::TAU);
            let delta = [distance * sine(angle), -distance * cosine(angle)];
            let trial = [
                self.origin[0] + delta[0],
                self.origin[1] + delta[1],
                self.origin[2],
            ];
            if self.clear_at(trial, mode)
                && let Some(face) = self.face(trial, mode)
            {
                let surface = face.ground(trial, self.tables, mode);
                let [nx, ny, nz] = surface.normal;
                let pitch = -ny.clamp(-1., 1.).asin();
                let roll = nx.atan2(nz);
                return Ok(Motion {
                    delta: [
                        delta[0] * roll.cos(),
                        delta[0] * roll.sin() * pitch.sin() + delta[1] * pitch.cos(),
                        face.correction(trial),
                    ],
                    response: Some(surface.response),
                    slope: [pitch, roll, 0.],
                });
            }
        }
        let correction = self
            .faces
            .iter()
            .filter(|f| f.contains(self.origin))
            .map(|f| f.correction(self.origin))
            .fold(-self.origin[2], f32::max);
        Ok(Motion {
            delta: [0., 0., correction],
            response: None,
            slope: [0.; 3],
        })
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Motion {
    pub delta: [f32; 3],
    pub response: Option<i8>,
    /// Pitch/roll in radians, for terrain-aligned presentation.
    pub slope: [f32; 3],
}

// Native world locomotion uses these polynomials rather than platform sin/cos.
pub(super) fn sine(angle: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let (x, sign) = if angle <= PI {
        (
            if angle <= FRAC_PI_2 {
                angle
            } else {
                FRAC_PI_2 - (angle - FRAC_PI_2)
            },
            1.,
        )
    } else {
        (
            if angle < PI + FRAC_PI_2 {
                angle - PI
            } else {
                FRAC_PI_2 - (angle - (PI + FRAC_PI_2))
            },
            -1.,
        )
    };
    ((0.00761 * (x * x) - 0.16605) * (x * x) + 1.) * x * sign
}
pub(super) fn cosine(angle: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let (x, sign) = if angle <= PI {
        if angle <= FRAC_PI_2 {
            (angle, 1.)
        } else {
            (FRAC_PI_2 - (angle - FRAC_PI_2), -1.)
        }
    } else if angle < PI + FRAC_PI_2 {
        (angle - PI, -1.)
    } else {
        (FRAC_PI_2 - (angle - (PI + FRAC_PI_2)), 1.)
    };
    ((0.03705 * (x * x) - 0.4967) * (x * x) + 1.) * sign
}

#[cfg(test)]
pub(super) mod tests;
