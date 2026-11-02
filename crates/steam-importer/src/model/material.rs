//! Material/image bindings needed by the common GC material pass.
use super::*;

#[derive(Default)]
pub struct Materials {
    pub records: Vec<Material>,
    pub textures: Vec<Texture>,
}
pub struct Material {
    pub name: String,
    pub variation_flags: u16,
    pub parameter: Option<usize>,
    pub textures: Vec<Slot>,
}
#[derive(Clone)]
pub struct Slot {
    pub texture: u16,
}
pub struct Texture {
    pub name: String,
    pub wrap: [resonance_content::TextureWrap; 2],
}

fn wrap(flags: u16) -> [resonance_content::TextureWrap; 2] {
    use resonance_content::TextureWrap;
    [0, 1].map(|axis| {
        if flags & (1 << axis) != 0 {
            TextureWrap::Mirror
        } else if flags & (4 << axis) != 0 {
            TextureWrap::Repeat
        } else {
            TextureWrap::Clamp
        }
    })
}

pub(super) fn read(source: &Reader<'_>, section: usize) -> Result<Materials> {
    source.slice(section, 80)?;
    let records = source.table(section + 24, 40)?;
    let assignments = source.table(section + 32, 6)?;
    let states = source.table(section + 40, 20)?;
    let textures = source.table(section + 72, 8)?;
    ensure!(
        records.count == states.count,
        "material state count mismatch"
    );
    let slots = (0..assignments.count)
        .map(|i| {
            let at = assignments.at(i);
            let texture = source.half(at + 2)?;
            ensure!(
                usize::from(texture) < textures.count,
                "material texture out of bounds"
            );
            Ok(Slot { texture })
        })
        .collect::<Result<Vec<_>>>()?;
    let records = (0..records.count)
        .map(|i| {
            let at = records.at(i);
            let count = source.half(at + 8)? as usize;
            let first = source.half(at + 12)? as usize;
            ensure!(count <= 8, "too many material texture stages");
            let parameter = if source.signed(at + 18)? == 0 {
                None
            } else {
                let parameters = source.table(at + 32, 4)?;
                ensure!(parameters.count >= 5, "missing material parameter binding");
                Some(source.word(parameters.at(4))? as usize)
            };
            Ok(Material {
                name: source.name(states.at(i) + 16)?,
                variation_flags: source.half(at + 4)?,
                parameter,
                textures: slots
                    .get(first..first + count)
                    .context("material textures exceed directory")?
                    .to_vec(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let textures = (0..textures.count)
        .map(|i| {
            let at = textures.at(i);
            let name = source.name(at)?;
            crate::source::validate_name(&format!("{name}.totexb_d"))?;
            Ok(Texture {
                name,
                wrap: wrap(source.half(at + 4)?),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Materials { records, textures })
}
