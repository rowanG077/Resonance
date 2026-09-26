//! Presentation inputs share resource handles with the prepared simulation.
use anyhow::{Context, Result};
use resonance_content::{
    animation::Skeleton,
    battle_effect::{EffectTexture, SourceBank, Tints},
    battle_model::TrailMaterial,
    font::UiTexture,
    model_preview::PreviewPart,
    texture::Sampler,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct RenderModel {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub parts: Vec<PreviewPart>,
    pub capacity: usize,
    pub lit: bool,
    /// Source particle declarations choose per-instance material state.
    pub effect: bool,
    pub texture_channels: Vec<resonance_content::battle_profile::TextureChannel>,
    pub weapon_flags: Option<u8>,
    /// 5B528 suppresses only the named node's mesh, preserving its transform.
    pub suppressed_nodes: BTreeSet<usize>,
}

/// 40C8/44388 invoke 5B528 unless profile+5C bit1000 retains body attachments.
/// Primary and outline share these indexed node names and the suppression.
pub(super) fn suppressed_body_nodes(skeleton: &Skeleton, flags: u32) -> BTreeSet<usize> {
    if flags & 0x1000 != 0 {
        return BTreeSet::new();
    }
    skeleton
        .bones
        .iter()
        .enumerate()
        .filter_map(|(index, bone)| {
            let prefix = bone.name.as_bytes().get(..2).unwrap_or_default();
            (prefix.eq_ignore_ascii_case(b"kk") || prefix.eq_ignore_ascii_case(b"pa"))
                .then_some(index)
        })
        .collect()
}

pub struct RenderEffect {
    pub resource: u32,
    pub source: SourceBank,
    pub textures: BTreeMap<u8, Vec<resonance_content::battle_effect::EffectTexture>>,
    /// Particle declarations reached by the selected original programs.
    pub members: BTreeSet<u16>,
    pub palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
}

pub struct RenderTrail {
    pub resource: u32,
    pub material: TrailMaterial,
    pub texture: Option<(UiTexture, Sampler)>,
    pub capacity: usize,
}

pub(super) fn effects(
    resource: u32,
    source: &SourceBank,
    programs: &BTreeSet<u16>,
    direct: &[u16],
    tints: &Tints,
    elements: &BTreeSet<u8>,
) -> Result<RenderEffect> {
    let mut members: BTreeSet<_> = direct.iter().copied().collect();
    for &member in programs {
        members.extend(
            source
                .program(usize::from(member))?
                .particles
                .keys()
                .map(|&id| u16::from(id)),
        );
    }
    let mut palettes = BTreeMap::new();
    for &member in &members {
        let particle = source.particle(usize::from(member))?;
        if particle.element_tint {
            let mut pairs = BTreeSet::new();
            for &element in elements {
                let palette = *tints
                    .palettes
                    .get(usize::from(element))
                    .context("invalid effect element")?;
                pairs.insert([palette, particle.state.palettes[1]]);
            }
            palettes.insert(member, pairs);
        }
    }
    Ok(RenderEffect {
        resource,
        source: source.clone(),
        textures: source
            .art
            .as_ref()
            .context("missing battle effect artwork")?
            .textures
            .clone(),
        members,
        palettes,
    })
}

pub(super) fn trail(
    resource: u32,
    material: TrailMaterial,
    common: &SourceBank,
    enemy: Option<&SourceBank>,
) -> Result<RenderTrail> {
    let texture = if material.texture < 0 {
        None
    } else {
        // 44E1C registers an enemy's relative slot 2 at 2 + resource index;
        // 47FA4 selects that generation from the owner. Common slots stay shared.
        let bank = if material.texture == 2 {
            enemy.context("missing weapon trail enemy bank")?
        } else {
            common
        };
        let image = bank
            .art
            .as_ref()
            .and_then(|art| art.textures.get(&(material.texture as u8)))
            .and_then(|textures| textures.first())
            .context("missing weapon trail texture")?;
        let page = trail_image(image, material.palette)?;
        Some((page.clone(), image.sampler.clone()))
    };
    Ok(RenderTrail {
        resource,
        material,
        texture,
        capacity: 1,
    })
}

fn trail_image(texture: &EffectTexture, palette: u8) -> Result<&UiTexture> {
    // 769DC passes no stride override. 4AEFC advances 16 TLUT entries for
    // CI4 and 256 for CI8, independently of cooked page spacing. Direct formats do not
    // consume the TLUT selector and have one image regardless of its value.
    let page = texture.palette_page(palette, 0)?;
    texture
        .images
        .get(page)
        .context("missing weapon trail palette")
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        TextureWrap,
        battle_effect::Art,
        texture::{Filter, TextureLod},
    };

    fn texture(name: &str, step: u16, pages: usize) -> EffectTexture {
        EffectTexture {
            palette_step: step,
            page_step: step,
            images: (0..pages)
                .map(|page| UiTexture {
                    path: format!("{name}-{page}.ktx2"),
                    width: 512,
                    height: 512,
                })
                .collect(),
            sampler: Sampler {
                wrap: [TextureWrap::Clamp; 2],
                min_filter: Filter::Linear,
                mag_filter: Filter::Linear,
                lod: TextureLod::default(),
            },
        }
    }

    #[test]
    fn trail_palette_uses_native_format_page_stride() -> Result<()> {
        // Both native formats address palette 2 at different byte offsets,
        // but publication has already split those windows into page 2.
        for step in [16, 256] {
            let texture = texture("indexed", step, 3);
            assert_eq!(trail_image(&texture, 0)?.path, "indexed-0.ktx2");
            assert_eq!(trail_image(&texture, 2)?.path, "indexed-2.ktx2");
            assert!(trail_image(&texture, 3).is_err());
        }
        assert_eq!(
            trail_image(&texture("direct", 0, 1), 255)?.path,
            "direct-0.ktx2"
        );
        assert!(trail_image(&texture("empty", 0, 0), 0).is_err());
        assert!(trail_image(&texture("ci14", 16384, 1), 1).is_err());
        Ok(())
    }

    #[test]
    fn body_attachment_suppression_keeps_transforms_and_respects_profile_flag() {
        use resonance_content::animation::{Bone, Transform, TransformChannels};
        let skeleton = Skeleton {
            bones: [
                "root",
                "kk00",
                "Kk01",
                "pa00",
                "PA_attachment",
                "at00",
                "k",
                "hand",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, name)| Bone {
                name: name.into(),
                parent: (index != 0).then_some(0),
                bind_channels: TransformChannels(8),
                bind: Transform::default(),
            })
            .collect(),
        };
        let before = skeleton.bind_pose().unwrap().global;
        assert_eq!(
            suppressed_body_nodes(&skeleton, 0),
            BTreeSet::from([1, 2, 3, 4])
        );
        assert_eq!(
            suppressed_body_nodes(&skeleton, 0x8000),
            BTreeSet::from([1, 2, 3, 4])
        );
        assert!(suppressed_body_nodes(&skeleton, 0x1000).is_empty());
        assert_eq!(skeleton.bind_pose().unwrap().global, before);
    }

    fn bank(textures: BTreeMap<u8, Vec<EffectTexture>>) -> SourceBank {
        SourceBank {
            source_sha256: String::new(),
            programs: vec![],
            actors: vec![],
            modifiers: BTreeMap::new(),
            uv: vec![],
            uv_roots: vec![],
            art: Some(Art {
                source_sha256: String::new(),
                textures,
                models: BTreeMap::new(),
                files: BTreeMap::new(),
            }),
        }
    }

    #[test]
    fn enemy_trail_selects_its_verified_owner_atlas() -> Result<()> {
        let common = bank(BTreeMap::from([
            (0, vec![texture("common", 16, 2)]),
            (2, vec![texture("wrong-common", 16, 2)]),
        ]));
        let enemy = bank(BTreeMap::from([
            (0, vec![texture("wrong-enemy", 16, 2)]),
            (2, vec![texture("owner", 256, 2)]),
        ]));
        let mut material = TrailMaterial {
            texture: 2,
            palette: 1,
            flags: 1,
            color: [64; 3],
            uv: [0, 0, 64, 64],
        };
        assert_eq!(
            trail(1, material, &common, Some(&enemy))?
                .texture
                .unwrap()
                .0
                .path,
            "owner-1.ktx2"
        );
        assert!(trail(1, material, &common, None).is_err());
        material.texture = 0;
        assert_eq!(
            trail(1, material, &common, Some(&enemy))?
                .texture
                .unwrap()
                .0
                .path,
            "common-1.ktx2"
        );
        material.texture = -1;
        assert!(trail(1, material, &common, None)?.texture.is_none());
        Ok(())
    }
}
