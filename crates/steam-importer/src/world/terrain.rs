//! Named terrain tiles, collision and GC-style water animation.
use super::*;
use resonance_content::TextureAnimation;

pub(super) fn filename(template: &str, row: u8, column: u8) -> Result<String> {
    let parts = template.split("%c").collect::<Vec<_>>();
    ensure!(
        parts.len() == 3 && parts.iter().all(|part| !part.contains('%')),
        "invalid terrain filename template"
    );
    let name = format!(
        "{}{}{}{}{}",
        parts[0], row as char, parts[1], column as char, parts[2]
    );
    crate::source::validate_name(&name)?;
    Ok(name)
}

struct Animations(BTreeMap<String, [u8; 6]>);
impl Animations {
    fn read(source: &Source, image: &pe::Image<'_>) -> Result<Self> {
        let name = image.text(0x86ffe8)?;
        ensure!(
            name.eq_ignore_ascii_case("fieldanm.bin"),
            "world water directory changed"
        );
        let bytes = source.require(name)?;
        ensure!(
            bytes.len() == 231 * 16,
            "unexpected world texture directory length"
        );
        let mut slots = BTreeMap::new();
        for (index, record) in bytes.chunks_exact(16).enumerate() {
            let name = image.slice(0x7a2e70 + index as u32 * 8, 8)?;
            let end = name
                .iter()
                .position(|&b| b == 0)
                .context("unterminated world terrain stem")?;
            let name = std::str::from_utf8(&name[..end])?.to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            ensure!(
                name.bytes().all(|b| b.is_ascii_alphanumeric()),
                "invalid world terrain stem"
            );
            ensure!(
                slots.insert(name, record[8..14].try_into()?).is_none(),
                "duplicate world terrain stem"
            );
        }
        Ok(Self(slots))
    }

    fn tracks(
        &self,
        source: &Source,
        package: &str,
        descriptor: &str,
        native: &crate::model::Model,
    ) -> Result<Vec<TextureAnimation>> {
        let stem = package
            .rsplit_once('.')
            .context("world tile has no extension")?
            .0
            .to_ascii_lowercase();
        let slots = self
            .0
            .get(&stem)
            .context("world terrain has no water record")?;
        if slots.iter().all(|&slot| slot == 0) {
            return Ok(Vec::new());
        }
        let material_name = format!(
            "{}.mxd",
            descriptor
                .rsplit_once('.')
                .context("terrain model has no extension")?
                .0
        );
        let bytes = source.require(&material_name)?;
        ensure!(
            bytes.get(..4) == Some(b"MXD "),
            "invalid terrain material parameters"
        );
        let mut at = pe::word(&bytes, 8)? as usize;
        let mut channels = None;
        for _ in 0..pe::word(&bytes, 12)? {
            let length = pe::word(&bytes, at + 4)? as usize;
            ensure!(length >= 16, "invalid terrain material chunk");
            let chunk = bytes
                .get(at..at + length)
                .context("terrain material chunk exceeds source")?;
            if chunk.get(..4) == Some(b"CTAM") {
                let start = pe::word(chunk, 16)? as usize;
                let count = pe::word(chunk, 20)? as usize;
                let mut values = Vec::new();
                for index in 0..count {
                    let row = start + index * 16;
                    let size = pe::word(chunk, row + 12)?;
                    values.push(if size == 0 {
                        None
                    } else {
                        ensure!(size == 192, "unsupported terrain material profile");
                        let offset = pe::word(chunk, row + 8)? as usize;
                        Some(
                            *chunk
                                .get(offset + 0x61)
                                .context("missing terrain UV channel")?,
                        )
                    });
                }
                ensure!(
                    channels.replace(values).is_none(),
                    "duplicate terrain material directory"
                );
            }
            at = at
                .checked_add(length)
                .context("terrain material chunk overflow")?;
        }
        let channels = channels.context("terrain material directory is missing")?;
        let mut images = BTreeMap::<u8, BTreeSet<usize>>::new();
        for material in &native.materials.records {
            let Some(index) = material.parameter else {
                continue;
            };
            let channel = channels
                .get(index)
                .copied()
                .flatten()
                .context("terrain material has no UV channel")?;
            for texture in &material.textures {
                images
                    .entry(channel)
                    .or_default()
                    .insert(usize::from(texture.texture));
            }
        }
        let mut result = Vec::new();
        for (index, &slot) in slots.iter().enumerate() {
            let Some(channel) = slot.checked_sub(1) else {
                continue;
            };
            let targets = images
                .get(&channel)
                .context("terrain animated UV channel has no image")?;
            for &texture in targets {
                let offsets = if index < 2 {
                    (0..1000).map(|tick| [tick as f32 * 0.001; 2]).collect()
                } else {
                    (0..160)
                        .map(|tick| [0., (tick / 20) as f32 * 0.125])
                        .collect()
                };
                let track = TextureAnimation {
                    texture,
                    delay_ticks: 0,
                    loop_start: 0,
                    offsets,
                };
                track.validate()?;
                if let Some(old) = result
                    .iter()
                    .find(|old: &&TextureAnimation| old.texture == texture)
                {
                    ensure!(
                        old.delay_ticks == track.delay_ticks
                            && old.loop_start == track.loop_start
                            && old.offsets == track.offsets,
                        "terrain image has conflicting UV channels"
                    );
                } else {
                    result.push(track);
                }
            }
        }
        Ok(result)
    }
}

fn tile(
    source: &Source,
    library: &Path,
    animations: &Animations,
    resource: &str,
    owner: &str,
    alternate: bool,
) -> Result<()> {
    let bytes = source.require(resource)?;
    let package = Package::parse(&bytes)?;
    ensure!(
        package.len() == 7 && package.member(5)?.is_none() && package.member(6)?.is_none(),
        "invalid world tile package"
    );
    let collision = source.require(
        package
            .reference(4)?
            .context("world tile has no collision")?,
    )?;
    let mut groups = crate::field_core::collision_groups(&collision, false)?;
    for group in &mut groups {
        group.surface = surface(group.surface)?.index() as u32;
    }
    ensure!(!groups.is_empty(), "world tile collision is empty");
    let prefix = if alternate { "alternate-" } else { "" };
    let path = format!("{owner}/{prefix}collision.glb");
    output::write(
        &library.join(&path),
        &world::collision_mesh::encode(&groups)?,
    )?;
    let mut parts = Vec::new();
    for slot in [0, 2] {
        let Some(body) = package.reference(slot)? else {
            continue;
        };
        let motions = package
            .reference(slot + 1)?
            .map(|name| vec![(0, name.to_owned())])
            .unwrap_or_default();
        let (mut part, native) = preview_model::part(
            source,
            library,
            body,
            owner,
            &format!("{prefix}part-{}", parts.len()),
            &motions,
        )?;
        part.resource = slot as u16;
        part.autoplay = !part.clips.is_empty();
        for material in &mut part.materials {
            material.depth_write = slot == 0;
        }
        if slot == 2 {
            part.texture_animations = animations.tracks(source, resource, body, &native)?;
        }
        parts.push(part);
    }
    output::json(
        &library.join(owner).join(if alternate {
            "alternate.json"
        } else {
            "terrain.json"
        }),
        &document::Terrain {
            collision: path,
            parts,
        },
    )
}

pub(super) fn cook(source: &Source, library: &Path, image: &pe::Image<'_>) -> Result<()> {
    let animations = Animations::read(source, image)?;
    let templates = [0x86ffa0, 0x86ffac, 0x86ffb8, 0x86ffc4]
        .map(|address| image.text(address))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let axis = image.text(0x919494)?;
    ensure!(
        axis.len() >= world::TILE_COLUMNS && axis.bytes().all(|b| b.is_ascii_alphanumeric()),
        "invalid world tile axis"
    );
    let mut mappings = BTreeMap::new();
    let mut count = [0, 0];
    for (index, name) in ["sylvarant", "tethe-alla"].into_iter().enumerate() {
        let mut terrain = Vec::new();
        for row in 0..world::TILE_ROWS {
            for column in 0..world::TILE_COLUMNS {
                let owner = format!("world/{name}/terrain/column-{column:02}-row-{row:02}");
                let base = filename(
                    templates[index * 2],
                    axis.as_bytes()[row],
                    axis.as_bytes()[column],
                )?;
                tile(source, library, &animations, &base, &owner, false)
                    .with_context(|| format!("world {name} tile {column},{row}"))?;
                count[0] += 1;
                let alternate = filename(
                    templates[index * 2 + 1],
                    axis.as_bytes()[row],
                    axis.as_bytes()[column],
                )?;
                let alternate = if source.contains(&alternate)? {
                    tile(source, library, &animations, &alternate, &owner, true)?;
                    count[1] += 1;
                    Some(format!("{owner}/alternate.json"))
                } else {
                    None
                };
                terrain.push(document::Tile {
                    column: column as u8,
                    row: row as u8,
                    base: format!("{owner}/terrain.json"),
                    alternate,
                });
            }
        }
        let continent = document::Continent { terrain };
        let path = format!("world/{name}/world.json");
        continent.validate(&path)?;
        output::json(&library.join(path), &continent)?;
        mappings.insert(index as u32, name.to_owned());
    }
    output::json(&library.join(document::MAPPINGS), &Mappings::new(mappings)?)?;
    eprintln!(
        "  World terrain: {} base tiles and {} story variants",
        count[0], count[1]
    );
    Ok(())
}
