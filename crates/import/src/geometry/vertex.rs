//! Resource-owned vertex formats. Unwritten caller state remains unknown.
use super::*;

#[derive(Clone, Copy, Default)]
struct Vat {
    words: [u32; 3],
    known: [u32; 3],
}

impl Vat {
    fn set(&mut self, word: usize, shift: u32, bits: u32, value: u32) {
        let mask = ((1 << bits) - 1) << shift;
        self.words[word] = (self.words[word] & !mask) | ((value << shift) & mask);
        self.known[word] |= mask;
    }

    fn get(&self, word: usize, shift: u32, bits: u32) -> Result<u32, GeometryError> {
        let mask = (1 << bits) - 1;
        if self.known[word] >> shift & mask != mask {
            return Err(Gpl("draw requires an inherited vertex format".into()));
        }
        Ok(self.words[word] >> shift & mask)
    }

    fn coordinate(
        &self,
        word: usize,
        shift: u32,
        fraction: (usize, u32),
        position: bool,
    ) -> Result<Format, GeometryError> {
        let components = self.get(word, shift, 1)? as usize + if position { 2 } else { 1 };
        let component = ComponentFormat::parse((self.get(word, shift + 1, 3)? as u8) << 4)?;
        let fraction = if component.width() == 1 && self.get(0, 30, 1)? == 0 {
            0
        } else {
            self.get(fraction.0, fraction.1, 5)? as u8
        };
        let format = VectorFormat::with_fraction(component, fraction, VectorKind::Coordinate);
        Ok(Format::Coordinate { format, components })
    }

    fn format(&self, attr: u8) -> Result<Format, GeometryError> {
        Ok(match attr {
            0 => Format::Matrix,
            9 => self.coordinate(0, 0, (0, 4), true)?,
            10 => Format::Normal {
                format: VectorFormat::new((self.get(0, 10, 3)? as u8) << 4, VectorKind::Normal)?,
                basis: self.get(0, 9, 1)? != 0,
                index3: self.get(0, 31, 1)? != 0,
            },
            11 | 12 => Format::Color(ColorFormat::parse(
                (self.get(0, 14 + u32::from(attr - 11) * 4, 3)? as u8) << 4,
            )?),
            13..=20 => {
                let (word, shift, fraction) = texture_fields(attr - 13);
                self.coordinate(word, shift, fraction, false)?
            }
            _ => return Err(Gpl(format!("unbound vertex attribute {attr}"))),
        })
    }
}

fn texture_fields(slot: u8) -> (usize, u32, (usize, u32)) {
    match slot {
        0 => (0, 21, (0, 25)),
        1..=3 => {
            let shift = u32::from(slot - 1) * 9;
            (1, shift, (1, shift + 4))
        }
        4 => (1, 27, (2, 0)),
        5..=7 => {
            let shift = 5 + u32::from(slot - 5) * 9;
            (2, shift, (2, shift + 4))
        }
        _ => unreachable!(),
    }
}

pub(super) struct Formats {
    tables: [Vat; 8],
    vcd: [u32; 2],
    revision: Option<usize>,
}

impl Formats {
    pub(super) fn new(arrays: &VertexArrays<'_>) -> Self {
        let mut result = Self {
            tables: [Vat::default(); 8],
            vcd: [0; 2],
            revision: None,
        };
        // SDK initialization enables byte dequantization as a revision bit;
        // all public attribute-format setters preserve it.
        for vat in &mut result.tables {
            vat.set(0, 30, 1, 1);
        }
        let vat = &mut result.tables[0];
        let position = arrays.positions.desc;
        vat.set(
            0,
            0,
            9,
            1 | (u32::from(position.format >> 4) << 1) | (u32::from(position.format & 15) << 4),
        );
        if let Some((normal, _)) = arrays.normals {
            let basis = normal.components == 2;
            vat.set(
                0,
                9,
                4,
                u32::from(basis) | (u32::from(normal.format >> 4) << 1),
            );
            vat.set(0, 31, 1, u32::from(basis));
        }
        if let Some(color) = arrays.colors.filter(|array| array.desc.count != 1) {
            vat.set(
                0,
                13,
                4,
                u32::from(color.desc.components != 3) | (u32::from(color.desc.format >> 4) << 1),
            );
        }
        for (slot, array) in arrays.texcoords.iter().enumerate() {
            let (word, shift, fraction) = texture_fields(slot as u8);
            vat.set(word, shift, 4, 1 | (u32::from(array.desc.format >> 4) << 1));
            vat.set(fraction.0, fraction.1, 5, u32::from(array.desc.format & 15));
        }
        result
    }

    pub(super) fn bind(&mut self, state: &RenderStateInfo, specs: Vec<VertexAttributeSpec>) {
        if self.revision == Some(state.vertex_revision) {
            return;
        }
        self.vcd = [0; 2];
        for spec in specs {
            match spec.attr {
                0 => self.vcd[0] |= 1,
                9..=12 => self.vcd[0] |= u32::from(spec.kind) << (9 + (spec.attr - 9) * 2),
                13..=20 => self.vcd[1] |= u32::from(spec.kind) << ((spec.attr - 13) * 2),
                _ => unreachable!(),
            }
        }
        self.revision = Some(state.vertex_revision);
    }

    pub(super) fn write(&mut self, register: u8, value: u32) -> Result<(), GeometryError> {
        match register {
            0x50 => self.vcd[0] = value,
            0x60 => self.vcd[1] = value,
            0x70..=0x77 | 0x80..=0x87 | 0x90..=0x97 => {
                let table = &mut self.tables[usize::from(register & 7)];
                let word = usize::from((register >> 4) - 7);
                table.words[word] = value;
                table.known[word] = u32::MAX;
            }
            0xa0..=0xbf => {
                return Err(Gpl(
                    "display list changes an external vertex array binding".into()
                ));
            }
            _ => return Err(Gpl(format!("unsupported CP register {register:#04x}"))),
        }
        Ok(())
    }

    pub(super) fn inputs(&self, table: u8) -> Result<Vec<Input>, GeometryError> {
        if self.vcd[0] & 0x1fe != 0 {
            return Err(Gpl(
                "per-vertex texture matrices need a transform consumer".into()
            ));
        }
        let mut inputs = Vec::new();
        for attr in [0].into_iter().chain(9..=20) {
            let kind = match attr {
                0 => self.vcd[0] & 1,
                9..=12 => self.vcd[0] >> (9 + (attr - 9) * 2) & 3,
                _ => self.vcd[1] >> ((attr - 13) * 2) & 3,
            } as u8;
            if kind != 0 {
                inputs.push(Input {
                    attr,
                    kind,
                    format: self.tables[usize::from(table)].format(attr)?,
                });
            }
        }
        if !inputs.iter().any(|input| input.attr == 9) {
            return Err(Gpl("vertex layout has no position".into()));
        }
        Ok(inputs)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Format {
    Matrix,
    Coordinate {
        format: VectorFormat,
        components: usize,
    },
    Normal {
        format: VectorFormat,
        basis: bool,
        index3: bool,
    },
    Color(ColorFormat),
}

#[derive(Clone, Copy)]
pub(super) struct Input {
    pub attr: u8,
    pub kind: u8,
    pub format: Format,
}

impl Input {
    pub(super) fn width(self) -> usize {
        if self.kind != 1 {
            return index_width(self.kind)
                * if matches!(
                    self.format,
                    Format::Normal {
                        basis: true,
                        index3: true,
                        ..
                    }
                ) {
                    3
                } else {
                    1
                };
        }
        match self.format {
            Format::Matrix => 1,
            Format::Coordinate { format, components } => format.component.width() * components,
            Format::Normal { format, basis, .. } => {
                format.component.width() * if basis { 9 } else { 3 }
            }
            Format::Color(format) => format.width(),
        }
    }

    pub(super) fn coordinate<const N: usize>(
        self,
        data: &[u8],
        at: usize,
        array: Option<VertexArray<'_, N>>,
        source: Option<&[u8]>,
    ) -> Result<[f32; N], GeometryError> {
        let Format::Coordinate { format, components } = self.format else {
            unreachable!()
        };
        let index = (self.kind != 1).then(|| read_index(data, at, index_width(self.kind)));
        let (data, at) = if let Some(index) = index {
            let array = array.ok_or_else(|| Gpl("unbound coordinate array".into()))?;
            if index >= array.desc.count {
                return Err(Gpl("coordinate index exceeds source array".into()));
            }
            if let Some(source) = source {
                let stride = (usize::from(array.desc.components)
                    * ComponentFormat::parse(array.desc.format)?.width())
                    & 255;
                (source, array.desc.data_offset + index * stride)
            } else {
                return indexed_vertex(Some(array.values), index, "coordinate");
            }
        } else {
            (data, at)
        };
        bounded(data, at, format.component.width() * components)?;
        let mut value = [0.; N];
        for (axis, slot) in value.iter_mut().take(components).enumerate() {
            *slot = format.read::<1>(data, at + axis * format.component.width())?[0];
        }
        Ok(value)
    }
}

pub(super) fn bounded(data: &[u8], at: usize, size: usize) -> Result<(), GeometryError> {
    if at.checked_add(size).is_none_or(|end| end > data.len()) {
        return Err(Gpl("vertex value exceeds source bytes".into()));
    }
    Ok(())
}
