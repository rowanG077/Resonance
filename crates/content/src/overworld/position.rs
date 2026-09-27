use super::{TILE_COLUMNS, TILE_ROWS, TILE_SIZE, WORLD_DEPTH, WORLD_WIDTH};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum World {
    Sylvarant,
    TetheAlla,
}
impl World {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Canonical map coordinates increase east/south. Height is independent of the
/// map. Construction and deserialization reject nonfinite positions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f32; 3]", into = "[f32; 3]")]
pub struct Position([f32; 3]);
impl TryFrom<[f32; 3]> for Position {
    type Error = anyhow::Error;
    fn try_from(point: [f32; 3]) -> Result<Self> {
        Self::from_map(point)
    }
}
impl From<Position> for [f32; 3] {
    fn from(value: Position) -> Self {
        value.map()
    }
}
impl Position {
    pub fn from_map(mut point: [f32; 3]) -> Result<Self> {
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "nonfinite world position"
        );
        point[0] = wrap(point[0], WORLD_WIDTH);
        point[1] = wrap(point[1], WORLD_DEPTH);
        Ok(Self(point))
    }

    /// Terrain/actor coordinates: X, horizontal Z, height. Tile (0,0) is centered
    /// at the origin; the map's origin is its northwest corner.
    pub fn from_plane(point: [f32; 3]) -> Result<Self> {
        Self::from_map([
            point[0] + TILE_SIZE / 2.,
            TILE_SIZE / 2. - point[1],
            point[2],
        ])
    }
    pub fn map(self) -> [f32; 3] {
        self.0
    }
    pub fn plane(self) -> [f32; 3] {
        [
            self.0[0] - TILE_SIZE / 2.,
            TILE_SIZE / 2. - self.0[1],
            self.0[2],
        ]
    }
    pub fn translated(self, delta: [f32; 3]) -> Result<Self> {
        Self::from_map([
            self.0[0] + delta[0],
            self.0[1] - delta[1],
            self.0[2] + delta[2],
        ])
    }
    pub fn tile(self) -> TileCoordinate {
        TileCoordinate {
            column: (self.0[0] / TILE_SIZE) as u8,
            row: (self.0[1] / TILE_SIZE) as u8,
        }
    }
    pub fn area_cell(self) -> [usize; 2] {
        [
            (self.0[0] / (TILE_SIZE / 2.)) as usize,
            (self.0[1] / (TILE_SIZE / 2.)) as usize,
        ]
    }
    pub fn local(self) -> [f32; 3] {
        let tile = self.tile();
        [
            self.0[0] - f32::from(tile.column) * TILE_SIZE - TILE_SIZE / 2.,
            TILE_SIZE / 2. - (self.0[1] - f32::from(tile.row) * TILE_SIZE),
            self.0[2],
        ]
    }
    /// Shortest displacement in the terrain plane, including both world seams.
    pub fn displacement_to(self, other: Self) -> [f32; 2] {
        [
            shortest(other.0[0] - self.0[0], WORLD_WIDTH),
            -shortest(other.0[1] - self.0[1], WORLD_DEPTH),
        ]
    }
    pub fn distance_to(self, other: Self) -> f32 {
        let [x, z] = self.displacement_to(other);
        x.hypot(z)
    }
}
fn wrap(value: f32, extent: f32) -> f32 {
    let result = value.rem_euclid(extent);
    // Floating point remainder can round a very small negative value to extent.
    if result >= extent { 0. } else { result }
}
fn shortest(value: f32, extent: f32) -> f32 {
    if value > extent / 2. {
        value - extent
    } else if value < -extent / 2. {
        value + extent
    } else {
        value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TileCoordinate {
    column: u8,
    row: u8,
}
impl TileCoordinate {
    pub fn new(column: u8, row: u8) -> Result<Self> {
        ensure!(
            usize::from(column) < TILE_COLUMNS && usize::from(row) < TILE_ROWS,
            "invalid world tile coordinate"
        );
        Ok(Self { column, row })
    }
    pub fn column(self) -> u8 {
        self.column
    }
    pub fn row(self) -> u8 {
        self.row
    }
    pub fn index(self) -> usize {
        usize::from(self.row) * TILE_COLUMNS + usize::from(self.column)
    }
    pub fn neighbor(self, [column, row]: [i8; 2]) -> Self {
        Self {
            column: (i16::from(self.column) + i16::from(column)).rem_euclid(TILE_COLUMNS as i16)
                as u8,
            row: (i16::from(self.row) + i16::from(row)).rem_euclid(TILE_ROWS as i16) as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_wrap_both_seams_and_keep_height() -> Result<()> {
        let northwest = Position::from_map([0., 0., 250.])?;
        assert_eq!(northwest.plane(), [-3200., 3200., 250.]);
        let across = northwest.translated([-1., 1., 0.])?;
        assert_eq!(across.map(), [76799., 57599., 250.]);
        assert_eq!(across.tile(), TileCoordinate::new(11, 8)?);
        assert_eq!(northwest.displacement_to(across), [-1., 1.]);
        assert_eq!(across.translated([1., -1., 0.])?, northwest);
        assert_eq!(northwest.tile().neighbor([-1, -1]), across.tile());
        assert_eq!(
            Position::from_map([-0.000001, -0.000001, 0.])?.tile(),
            northwest.tile()
        );
        for point in [
            [f32::NAN, 0., 0.],
            [0., f32::INFINITY, 0.],
            [0., 0., f32::NEG_INFINITY],
        ] {
            assert!(Position::from_map(point).is_err());
        }
        assert!(TileCoordinate::new(12, 0).is_err());
        Ok(())
    }
    #[test]
    fn every_tile_and_region_uses_the_same_origin() -> Result<()> {
        for row in 0..TILE_ROWS {
            for column in 0..TILE_COLUMNS {
                for (x, z) in [(0., 0.), (3199., 3199.), (3200., 3200.), (6399., 6399.)] {
                    let map = [
                        column as f32 * TILE_SIZE + x,
                        row as f32 * TILE_SIZE + z,
                        17.,
                    ];
                    let position = Position::from_map(map)?;
                    assert_eq!(position.tile().index(), row * TILE_COLUMNS + column);
                    assert_eq!(position.local(), [x - 3200., 3200. - z, 17.]);
                    assert_eq!(
                        position.area_cell(),
                        [
                            column * 2 + usize::from(x >= 3200.),
                            row * 2 + usize::from(z >= 3200.)
                        ]
                    );
                    assert_eq!(Position::from_plane(position.plane())?, position);
                }
            }
        }
        Ok(())
    }
}
