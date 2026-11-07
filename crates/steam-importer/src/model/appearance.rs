//! Native material atlas roles lower to the existing character selectors.
use super::*;
use crate::source::Source;
use resonance_content::{AppearanceTextures, AtlasChannel};

pub(crate) fn party_appearance(
    source: &Source,
    descriptor: &str,
    sizes: &[[u32; 2]],
) -> Result<AppearanceTextures> {
    let mut roles = atlas_roles(source, descriptor, sizes)?;
    // Raine's added costume omits role tags but retains unique standard face
    // atlas dimensions. Ambiguity remains an import error.
    for (role, size) in roles[..2].iter_mut().zip([[128, 1024], [256, 1024]]) {
        if role.is_none() {
            let matches = sizes
                .iter()
                .enumerate()
                .filter_map(|(index, &actual)| (actual == size).then_some(index))
                .collect::<Vec<_>>();
            ensure!(matches.len() == 1, "missing or ambiguous party face atlas");
            *role = Some(matches[0]);
        }
    }
    Ok(channels(roles, sizes))
}

/// Ordinary NPCs use the same native atlas tags, without party-only guesses
/// from image dimensions. Untagged props and effects keep their existing pass.
pub(crate) fn tagged_appearance(
    source: &Source,
    descriptor: &str,
    sizes: &[[u32; 2]],
) -> Result<Option<AppearanceTextures>> {
    let roles = atlas_roles(source, descriptor, sizes)?;
    Ok(roles
        .iter()
        .any(Option::is_some)
        .then(|| channels(roles, sizes)))
}

fn atlas_roles(
    source: &Source,
    descriptor: &str,
    sizes: &[[u32; 2]],
) -> Result<[Option<usize>; 4]> {
    let bytes = source.require(descriptor)?;
    let input = Reader(&bytes);
    let materials = material::read(&input, sections(&input)?[7])?;
    ensure!(
        materials.textures.len() == sizes.len(),
        "party texture palette differs"
    );
    let stem = descriptor
        .rsplit_once('.')
        .context("model has no extension")?
        .0;
    let (_, _, tags) = material_parameters(&source.require(&format!("{stem}.mxd"))?, &materials)?;
    let mut roles = [None; 4];
    for (material, role) in materials.records.iter().zip(tags) {
        if !(1..=4).contains(&role) {
            continue;
        }
        // The native MXD extension byte0x62 assigns eyes, mouth, body variant
        // and costume/Exsphere roles. Resolve the live material assignment;
        // old embedded texture indices can be stale in alternate costumes.
        let texture = usize::from(
            material
                .textures
                .iter()
                .find(|slot| slot.kind == 0)
                .context("party atlas material has no color texture")?
                .texture,
        );
        let target = &mut roles[usize::from(role - 1)];
        ensure!(
            target.is_none_or(|previous| previous == texture),
            "conflicting party atlas role"
        );
        *target = Some(texture);
    }
    Ok(roles)
}

fn channels(roles: [Option<usize>; 4], sizes: &[[u32; 2]]) -> AppearanceTextures {
    let variant = roles[2].and_then(|texture| {
        let [width, height] = sizes[texture];
        let frames = match (width == 128, height) {
            (true, 256) | (false, 512) => 2,
            (true, 512) | (false, 1024) => 4,
            _ => return None,
        };
        Some(AtlasChannel { texture, frames })
    });
    AppearanceTextures {
        eyes: roles[0],
        mouth: roles[1],
        variant,
        costume: roles[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untagged_npc_does_not_infer_face_channels_from_dimensions() {
        let sizes = [[128, 1024], [256, 1024]];
        let appearance = channels([None; 4], &sizes);
        assert_eq!(appearance.eyes, None);
        assert_eq!(appearance.mouth, None);
    }
}
