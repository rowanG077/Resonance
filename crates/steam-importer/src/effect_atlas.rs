//! Import-time description of the port's packed battle sprite images.
//!
//! The source rectangle is in the original effect's pixel coordinates. These
//! tables locate its pixels in a packed RGBA image; they do not define the
//! rendering style or replace the shared GameCube material contract.
use super::{output, package::Package, pe, source::Source, texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{
        EffectOperation, EffectTexture, declaration::Declaration, document::Definition,
        visual::ParticleTexture,
    },
    font::UiTexture,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) mod frames;

/// Native loader bindings. These slot numbers never reach the renderer as
/// edition-specific behavior; they select ordinary cooked artwork.
pub(crate) fn residents(source: &Source, image: &pe::Image<'_>) -> Result<BTreeMap<u8, String>> {
    for (address, bytes) in [
        (
            0x4c3116,
            &[0x68, 0x70, 9, 0x87, 0, 0xb9, 0xdc, 0xd7, 0x8e, 0][..],
        ),
        (0x46731e, &[0x8b, 0x1d, 0xec, 0xd7, 0x8e, 0]),
        (
            0x46732d,
            &[0xc7, 0x45, 0xe8, 10, 0, 11, 21, 0xc6, 0x45, 0xec, 1],
        ),
        (0x46738c, &[0x8b, 0x43, 0x14]),
        (0x4673b6, &[0x68, 10, 0, 0, 13]),
        (0x4673fe, &[0x0f, 0xbe, 0x44, 0x35, 0xe8]),
        (0x467412, &[0x89, 0x8c, 0x83, 0x4c, 0xb2, 0x15, 0]),
    ] {
        ensure!(
            image.slice(address, bytes.len())? == bytes,
            "Steam resident artwork consumer changed at {address:#x}"
        );
    }
    let bytes = source.require(image.text(0x870970)?)?;
    let package = Package::parse(&bytes)?;
    let directory = Package::parse(package.member(4)?.context("missing resident textures")?)?;
    ensure!(directory.len() == 5, "resident texture directory changed");
    [10, 0, 11, 21, 1]
        .into_iter()
        .enumerate()
        .map(|(member, slot)| {
            Ok((
                slot,
                directory
                    .reference(member)?
                    .context("missing resident texture")?
                    .to_owned(),
            ))
        })
        .collect()
}

/// Repack sprite frames into the GC image/palette contract. Until this step,
/// palette_set carries the import-only native palette stride, preventing the
/// shared decoder from interning declarations with different image bindings.
pub(crate) fn cook(
    source: &Source,
    library: &Path,
    bank: &mut Definition,
    name: &str,
    table: Option<&Table>,
    owner: &str,
    slot: u8,
) -> Result<EffectTexture> {
    cook_with_pixels(
        source,
        library,
        bank,
        name,
        table,
        owner,
        slot,
        |_, _, _| {},
    )
}

/// A narrowly selected artwork style may change pixels before interning pages.
/// Definitions and native UV remapping still pass through the ordinary compiler.
pub(crate) fn cook_with_pixels(
    source: &Source,
    library: &Path,
    bank: &mut Definition,
    name: &str,
    table: Option<&Table>,
    owner: &str,
    slot: u8,
    mut normalize: impl FnMut(usize, u16, &mut resonance_asset_writer::texture::Pixels),
) -> Result<EffectTexture> {
    let descriptor = texture::Descriptor::read(&source.require(name)?)?;
    let decoded = descriptor.decode(&source.require(&descriptor.payload)?)?;
    let mut texture = EffectTexture {
        images: Vec::new(),
        sampler: resonance_content::texture::Sampler {
            wrap: [resonance_content::TextureWrap::Clamp; 2],
            ..Default::default()
        },
        // Non-particle artwork (weapon trails and wing sparkles) uses set zero
        // in the shared renderer. Each repacked particle selects another set.
        palette_sets: vec![vec![None; 256]],
    };
    let mut hashes = BTreeMap::<[u8; 32], usize>::new();
    for member in 0..bank.templates.len() {
        let Ok(visual) = bank.templates[member].particle_visual() else {
            continue;
        };
        if visual.texture.slot() != Some(slot) {
            continue;
        }
        // A zero native stride uses sixteen bytes per selector.
        let stride = match visual.texture.palette_set() {
            0 => 16,
            stride => u16::from(stride),
        };
        let plan = frames::Plan::read(bank, member)
            .with_context(|| format!("sprite template {member}, slot {slot}, {name}"))?;
        let selectors = (0..256u16)
            .map(|selector| {
                let key = selector * stride / 16;
                if table.is_some_and(|t| t.rows.iter().any(|r| r.palette == key)) {
                    key
                } else {
                    u16::MAX
                }
            })
            .collect::<Vec<_>>();
        let mut indices = BTreeMap::new();
        for &key in &selectors {
            if indices.contains_key(&key) {
                continue;
            }
            let mut pixels = plan
                .image(&decoded.levels[0], table, key)
                .with_context(|| {
                    format!("sprite template {member}, slot {slot}, palette {key}, {name}")
                })?;
            normalize(member, key, &mut pixels);
            let bytes = resonance_asset_writer::ktx2::encode_rgba8(
                pixels.width,
                pixels.height,
                &pixels.rgba,
            )?;
            let hash: [u8; 32] = Sha256::digest(&bytes).into();
            let index = if let Some(&index) = hashes.get(&hash) {
                index
            } else {
                let index = texture.images.len();
                let path = format!("{owner}/textures/atlas-{slot}-{index}.ktx2");
                output::write(&library.join(&path), &bytes)?;
                texture.images.push(UiTexture {
                    path,
                    width: pixels.width,
                    height: pixels.height,
                });
                hashes.insert(hash, index);
                index
            };
            indices.insert(key, index);
        }
        let choices = selectors
            .iter()
            .map(|key| Some(indices[key]))
            .collect::<Vec<_>>();
        let set = texture
            .palette_sets
            .iter()
            .position(|old| *old == choices)
            .unwrap_or_else(|| {
                let set = texture.palette_sets.len();
                texture.palette_sets.push(choices);
                set
            });
        for event in bank.programs.iter_mut().flatten() {
            if let EffectOperation::Spawn {
                particle, birth, ..
            } = &mut event.operation
                && usize::from(*particle) == member
            {
                plan.rewrite_birth(birth)?;
            }
        }
        if let Declaration::Particle { template, visual } = &mut bank.templates[member] {
            *template = plan.template;
            // Steam's packed image already carries alpha in its RGBA texels.
            visual.texture = ParticleTexture::Atlas {
                slot,
                dual: false,
                palette_set: set.try_into()?,
            };
        }
    }
    ensure!(
        !texture.images.is_empty(),
        "effect atlas has no sprite images"
    );
    Ok(texture)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Remap {
    /// Native palette byte offset divided by sixteen, not the actor selector.
    pub palette: u16,
    pub source: [f32; 4],
    pub packed: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    pub rows: Vec<Remap>,
}

#[derive(Default)]
pub(crate) struct Directory(BTreeMap<String, Table>);

impl Directory {
    pub(crate) fn read(image: &pe::Image<'_>) -> Result<Self> {
        // The constructor installs 113 name/table pairs. Check every call and
        // destination as well as the terminator; a scan for plausible floats
        // would also admit unrelated tables in this part of the executable.
        const START: u32 = 0x6fb458;
        const END: u32 = 0x6fbd2c;
        ensure!(
            image.slice(END, 17)?
                == [
                    0x33, 0xc0, 0xa3, 0x78, 0x16, 0xd9, 1, 0xa3, 0x7c, 0x16, 0xd9, 1, 0x5f, 0x8b,
                    0xe5, 0x5d, 0xc3
                ],
            "Steam atlas directory terminator changed"
        );
        for (address, bytes) in [
            (0x594767, &[0xb9, 9, 0, 0, 0][..]),
            (0x594be5, &[0x43, 0x8d, 0x14, 0xdb, 0x83, 0x3c, 0x90, 0]),
            // The draw consumer passes palette byte offset >> 4.
            (0x594cf5, &[0xc1, 0xe8, 4, 0x68, 0x98, 2, 0xb2, 0]),
        ] {
            ensure!(
                image.slice(address, bytes.len())? == bytes,
                "Steam atlas consumer changed at {address:#x}"
            );
        }
        ensure!(
            f64::from_le_bytes(image.slice(0x882be8, 8)?.try_into()?) == f64::from(0.0001f32),
            "Steam atlas rectangle tolerance changed"
        );
        let mut directory = BTreeMap::new();
        for (index, address) in (START..END).step_by(20).enumerate() {
            let instruction = image.slice(address, 20)?;
            ensure!(
                [
                    instruction[0],
                    instruction[5],
                    instruction[10],
                    instruction[15]
                ] == [0x68, 0xb9, 0xbf, 0xe8]
                    && pe::word(instruction, 11)? == 0x1d912f0 + index as u32 * 8
                    && (address + 20).wrapping_add(pe::word(instruction, 16)?) == 0x594560,
                "Steam atlas initializer changed at {address:#x}"
            );
            let name = image.text(pe::word(instruction, 6)?)?;
            ensure!(
                name.ends_with(".TOTEXB_D"),
                "unexpected atlas resource {name}"
            );
            let table = Table::read(image, pe::word(instruction, 1)?)?;
            ensure!(
                directory.insert(name.to_ascii_uppercase(), table).is_none(),
                "duplicate atlas resource {name}"
            );
        }
        Ok(Self(directory))
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Table> {
        self.0.get(&name.to_ascii_uppercase())
    }
}

impl Table {
    fn read(image: &pe::Image<'_>, address: u32) -> Result<Self> {
        let mut rows = Vec::new();
        // Largest installed table has 991 rows. Bound malformed input even
        // when a caller uses the PE reader without the installed-build guard.
        for index in 0..4096 {
            let bytes = image.slice(address + index * 36, 36)?;
            let palette = pe::word(bytes, 0)? as i32;
            if palette < 0 {
                return Ok(Self { rows });
            }
            let coordinates = (1..9)
                .map(|i| Ok(f32::from_bits(pe::word(bytes, i * 4)?)))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                coordinates.iter().all(|v| v.is_finite()),
                "nonfinite atlas rectangle"
            );
            rows.push(Remap {
                palette: palette.try_into()?,
                source: coordinates[..4].try_into()?,
                packed: coordinates[4..].try_into()?,
            });
        }
        anyhow::bail!("unterminated Steam atlas table at {address:#x}")
    }

    /// Reference evaluator for cook-time normalization. Native 0x5946c0
    /// matches only corners zero and three, rewrites all four, and continues
    /// through the table: a later row may match the result of an earlier row.
    pub(crate) fn remap(&self, palette: u16, quads: &mut [[[f32; 2]; 4]]) {
        for row in &self.rows {
            if row.palette != palette {
                continue;
            }
            for quad in &mut *quads {
                let rectangle = [quad[0][0], quad[0][1], quad[3][0], quad[3][1]];
                if row
                    .source
                    .iter()
                    .zip(rectangle)
                    .all(|(a, b)| (a - b).abs() <= 0.0001f32)
                {
                    let [x1, y1, x2, y2] = row.packed;
                    *quad = [[x1, y1], [x2, y1], [x1, y2], [x2, y2]];
                }
            }
        }
    }
}
