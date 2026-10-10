//! Presentation inputs share resource handles with the prepared simulation.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::Skeleton,
    battle_effect::{SourceBank, Tints},
    battle_model::{TrailMaterial, TrailTexture},
    diagnostics::Diagnostics,
    font::UiTexture,
    model_preview::PreviewPart,
    texture::Sampler,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub struct RenderModel {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub parts: Vec<PreviewPart>,
    pub capacity: usize,
    pub lit: bool,
    /// Source particle declarations choose per-instance material state.
    pub effect: bool,
    pub texture_channels: Vec<resonance_content::battle_profile::TextureChannel>,
    pub weapon_style: Option<resonance_content::battle_profile::WeaponStyle>,
    /// Hide only the named mesh, preserving its transform.
    pub suppressed_nodes: BTreeSet<usize>,
}

/// Publish the sampled model and its drawing resources together, or omit this effect.
pub(super) fn effect_model(
    files: &resonance_content::prepared::Files,
    part: &resonance_content::battle_model::ModelPart,
    resource: u32,
) -> Result<Option<(resonance_battle::PreparedEffectModel, RenderModel)>> {
    let prepared = crate::battle::model::effect(files, part, resource).map(|model| {
        let parts = part
            .layers
            .iter()
            .cloned()
            .map(|mut layer| {
                // The sampled pose includes attachments and animation.
                layer.attached_to = None;
                layer.animation = None;
                layer
            })
            .collect();
        (
            model,
            RenderModel {
                resource,
                skeleton: part.rig.skeleton.clone(),
                parts,
                capacity: resonance_battle::MAX_PARTICLES,
                lit: true,
                effect: true,
                texture_channels: vec![],
                suppressed_nodes: BTreeSet::new(),
                weapon_style: None,
            },
        )
    });
    files.diagnostics().attempt("battle effect model", prepared)
}

/// Suppress body attachments unless the profile retains them; keep their transforms available.
pub(super) fn suppressed_body_nodes(
    skeleton: &Skeleton,
    show_attachments: bool,
) -> BTreeSet<usize> {
    if show_attachments {
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

/// Use facial texture selectors from the primary body appearance, retaining profile frame counts.
pub(super) fn party_texture_channels(
    channels: &[resonance_content::battle_profile::TextureChannel],
    primary: &resonance_content::ScenePart,
) -> Result<Vec<resonance_content::battle_profile::TextureChannel>> {
    let mut channels = channels.to_vec();
    if channels.is_empty() {
        return Ok(channels);
    }
    let appearance = primary
        .appearance
        .as_ref()
        .context("party body has no expression selectors")?;
    for (channel, texture) in channels.iter_mut().zip([appearance.eyes, appearance.mouth]) {
        let texture = texture.context("party body has no expression texture")?;
        ensure!(
            texture < primary.textures.len(),
            "party expression texture is outside the body palette"
        );
        channel.texture =
            u8::try_from(texture).context("party expression texture exceeds source byte")?;
    }
    Ok(channels)
}

pub struct RenderEffect {
    pub resource: u32,
    pub source: Arc<SourceBank>,
    pub textures: BTreeMap<u8, Vec<resonance_content::battle_effect::EffectTexture>>,
    /// Particle declarations reached by the selected programs.
    pub members: BTreeSet<u16>,
    pub palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
    pub blends: BTreeMap<u16, BTreeSet<u8>>,
}

pub struct RenderTrail {
    /// Primary weapon layer that supplies the live pose.
    pub resource: u32,
    pub bones: Vec<u16>,
    pub material: TrailMaterial,
    pub texture: Option<(UiTexture, Sampler)>,
}

pub(super) fn effects(
    resource: u32,
    source: &Arc<SourceBank>,
    programs: &[u16],
    tints: Option<&Tints>,
    diagnostics: &Diagnostics,
) -> Result<RenderEffect> {
    let mut members = BTreeSet::new();
    let mut tinted_members = BTreeSet::new();
    let mut palettes: BTreeMap<u16, BTreeSet<[u8; 2]>> = BTreeMap::new();
    let mut blends: BTreeMap<u16, BTreeSet<u8>> = BTreeMap::new();
    for &member in programs {
        let Some(program) = diagnostics.attempt(
            "battle effect rendering",
            source.program(usize::from(member)),
        )?
        else {
            continue;
        };
        for event in program {
            use resonance_content::battle_effect::EffectOperation;
            if let EffectOperation::Spawn {
                particle,
                blend,
                palette,
                birth,
            } = &event.operation
            {
                let member = u16::from(*particle);
                members.insert(member);
                if birth.element_tint {
                    tinted_members.insert(member);
                }
                if let Some(range) = birth.palette {
                    let low = range.min.round().clamp(0., 255.) as u8;
                    let high = range.max.round().clamp(0., 255.) as u8;
                    for palette in low..=high {
                        palettes.entry(member).or_default().insert([
                            palette,
                            source.particle(usize::from(*particle))?.state.palettes[1],
                        ]);
                    }
                }
                if let Some(palette) = palette {
                    palettes.entry(member).or_default().insert([
                        *palette,
                        source.particle(usize::from(*particle))?.state.palettes[1],
                    ]);
                }
                if let Some(blend) = blend {
                    blends.entry(member).or_default().insert(*blend);
                }
            }
        }
    }

    for &member in &members {
        let particle = source.particle(usize::from(member))?;
        if let Some(tints) = tints
            && (particle.element_tint || tinted_members.contains(&member))
        {
            let mut pairs = BTreeSet::new();
            for &palette in &tints.palettes {
                pairs.insert([palette, particle.state.palettes[1]]);
            }
            palettes.entry(member).or_default().extend(pairs);
        }
    }
    Ok(RenderEffect {
        resource,
        source: Arc::clone(source),
        textures: if members.is_empty() {
            // A sound/controller-only member has no particle material dependency.
            BTreeMap::new()
        } else {
            diagnostics
                .attempt(
                    "battle effect artwork",
                    source.art.as_ref().context("missing battle effect artwork"),
                )?
                .map(|art| art.textures.clone())
                .unwrap_or_default()
        },
        members,
        palettes,
        blends,
    })
}

/// Geometry and material form one optional cosmetic resource.
pub(super) fn weapon_trail(
    diagnostics: &Diagnostics,
    skeleton: &Skeleton,
    resource: u32,
    material: TrailMaterial,
    common: Option<&SourceBank>,
    enemy: Option<&SourceBank>,
) -> Result<Option<RenderTrail>> {
    let prepared = (|| {
        let Some(bones) = crate::battle::trail::weapon_endpoints(skeleton)? else {
            return Ok(None);
        };
        let texture = if let Some(texture) = material.texture {
            let (bank, slot) = match texture {
                TrailTexture::Common { slot } => (common, slot),
                TrailTexture::Enemy { slot } => (enemy, slot),
            };
            let image = bank
                .context("missing weapon trail effect bank")?
                .art
                .as_ref()
                .and_then(|art| art.textures.get(&slot))
                .and_then(|textures| textures.first())
                .context("missing weapon trail texture")?;
            let page = image.palette_image(0, material.palette)?;
            Some((image.images[page].clone(), image.sampler.clone()))
        } else {
            None
        };
        Ok(Some(RenderTrail {
            resource,
            bones,
            material,
            texture,
        }))
    })();
    Ok(diagnostics
        .attempt("battle weapon trail", prepared)?
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        TextureWrap,
        battle_effect::{Art, EffectTexture},
        texture::{Filter, TextureLod},
    };

    fn texture(name: &str, indexed: bool, pages: usize) -> EffectTexture {
        EffectTexture {
            palette_sets: vec![if indexed {
                (0..pages).map(Some).collect()
            } else {
                vec![Some(0); 256]
            }],
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
    fn invalid_effect_models_leave_other_resources_usable() -> Result<()> {
        use resonance_content::{
            animation::{Bone, Transform, TransformChannels},
            battle_model::{ModelPart, Rig},
            prepared::Files,
        };
        let mut part = ModelPart {
            rig: Rig {
                skeleton: Skeleton {
                    bones: vec![Bone {
                        name: "root".into(),
                        parent: None,
                        bind_channels: TransformChannels(0),
                        bind: Transform::default(),
                    }],
                },
                attachments: BTreeMap::new(),
            },
            layer_skeletons: BTreeMap::new(),
            layers: vec![PreviewPart {
                scene: resonance_content::ScenePart {
                    resource: 0,
                    mesh: "effect.glb".into(),
                    textures: vec![],
                    materials: vec![],
                    appearance: None,
                    translation: [0.; 3],
                    clips: vec![],
                    autoplay: false,
                    texture_animations: vec![],
                    bone_names: vec!["root".into()],
                    material_nodes: vec![],
                    outline_color: None,
                    secondary_motion: Default::default(),
                },
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: vec![],
            }],
        };
        let files = Files::new(Diagnostics::default());
        let (model, render) = effect_model(&files, &part, 1)?.unwrap();
        assert_eq!(model.resource(), render.resource);
        part.layers[0].scene.bone_names.clear();
        assert!(effect_model(&files, &part, 2)?.is_none());
        assert_eq!(files.diagnostics().entries().len(), 1);
        assert!(effect_model(&Files::default(), &part, 2).is_err());
        part.layers[0].scene.bone_names.push("root".into());
        assert!(effect_model(&files, &part, 3)?.is_some());
        Ok(())
    }

    #[test]
    fn unavailable_programs_do_not_block_nonvisual_programs() {
        use resonance_content::battle_effect::{EffectOperation, ScheduledEvent};
        let source = Arc::new(SourceBank {
            source_sha256: String::new(),
            programs: vec![vec![ScheduledEvent {
                at: 0,
                operation: EffectOperation::Shake {
                    duration: 4,
                    amplitude: 2,
                },
            }]],
            actors: vec![],
            art: None,
        });
        let diagnostics = Diagnostics::default();
        let prepared = effects(77, &source, &[0, 1], None, &diagnostics).unwrap();
        assert!(prepared.members.is_empty());
        assert!(prepared.textures.is_empty());
        assert_eq!(diagnostics.entries().len(), 1);
        assert!(effects(77, &source, &[0, 1], None, &Diagnostics::new(true)).is_err());
    }

    #[test]
    fn genis_expression_channels_use_eyes_without_animating_the_body_atlas() -> Result<()> {
        use resonance_content::battle_profile::TextureChannel;
        let channels = [
            TextureChannel {
                texture: 0,
                frames: 16,
            },
            TextureChannel {
                texture: 2,
                frames: 8,
            },
        ];
        let mut primary: resonance_content::ScenePart = serde_json::from_value(
            serde_json::json!({
                "resource": 0, "mesh": "body.glb", "textures": ["body", "mouth", "hair", "eyes"],
                "materials": [], "appearance": {"eyes": 3, "mouth": 1, "variant": null, "costume": 1},
                "translation": [0.,0.,0.], "clips": [], "autoplay": false, "texture_animations": []
            }),
        )?;
        let selected = party_texture_channels(&channels, &primary)?;
        assert_eq!(
            selected
                .iter()
                .map(|c| (c.texture, c.frames))
                .collect::<Vec<_>>(),
            [(3, 16), (1, 8)]
        );
        assert_eq!(channels[0].texture, 0, "the shared profile stays unchanged");
        assert!(party_texture_channels(&[], &primary)?.is_empty());
        primary.appearance.as_mut().unwrap().eyes = Some(primary.textures.len());
        assert!(party_texture_channels(&channels, &primary).is_err());
        primary.appearance = None;
        assert!(party_texture_channels(&channels, &primary).is_err());
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
            suppressed_body_nodes(&skeleton, false),
            BTreeSet::from([1, 2, 3, 4])
        );
        assert!(suppressed_body_nodes(&skeleton, true).is_empty());
        assert_eq!(skeleton.bind_pose().unwrap().global, before);
    }

    #[test]
    fn program_overrides_select_particle_materials_without_tints() {
        use resonance_content::battle_effect::{EffectOperation, ScheduledEvent};
        let mut source = bank(BTreeMap::from([(
            0,
            vec![texture("color", true, 64), texture("alpha", true, 32)],
        )]));
        source.actors.push(
            serde_json::from_str(include_str!(
                "../../../../content/tests/fixtures/material-particle.json"
            ))
            .unwrap(),
        );
        source.programs.push(vec![ScheduledEvent {
            at: 0,
            operation: EffectOperation::Spawn {
                particle: 0,
                blend: Some(0),
                palette: Some(26),
                birth: Default::default(),
            },
        }]);
        let source = Arc::new(source);
        let result = effects(77, &source, &[0], None, &Diagnostics::new(true)).unwrap();
        assert_eq!(result.palettes[&0], BTreeSet::from([[26, 1]]));
        assert_eq!(result.blends, BTreeMap::from([(0, BTreeSet::from([0]))]));
        assert_eq!(result.members, BTreeSet::from([0]));
        assert_eq!(source.actors[0].particle_visual().unwrap().blend, 1);
        assert_eq!(source.actors[0].template().unwrap().state.palettes, [9, 1]);
    }

    fn bank(textures: BTreeMap<u8, Vec<EffectTexture>>) -> SourceBank {
        SourceBank {
            source_sha256: String::new(),
            programs: vec![],
            actors: vec![],
            art: Some(Art {
                source_sha256: String::new(),
                textures,
                models: BTreeMap::new(),
                files: BTreeMap::new(),
            }),
        }
    }

    #[test]
    fn weapon_trails_require_a_complete_optional_resource() -> Result<()> {
        use resonance_content::animation::{Bone, Transform, TransformChannels};
        let skeleton = Skeleton {
            bones: ["KI00", "KI01"]
                .map(|name| Bone {
                    name: name.into(),
                    parent: None,
                    bind_channels: TransformChannels(0),
                    bind: Transform::default(),
                })
                .into(),
        };
        let common = bank(BTreeMap::from([(2, vec![texture("common", true, 2)])]));
        let enemy = bank(BTreeMap::from([(2, vec![texture("owner", true, 2)])]));
        let material = TrailMaterial {
            texture: Some(TrailTexture::Enemy { slot: 2 }),
            palette: 1,
            additive: true,
            color: [64; 3],
            uv: [0, 0, 64, 64],
        };
        let diagnostics = Diagnostics::default();
        let prepare = |diagnostics: &Diagnostics, enemy| {
            weapon_trail(diagnostics, &skeleton, 1, material, Some(&common), enemy)
        };
        let render = prepare(&diagnostics, Some(&enemy))?.unwrap();
        assert_eq!(render.texture.unwrap().0.path, "owner-1.ktx2");
        assert!(prepare(&diagnostics, None)?.is_none());
        assert_eq!(diagnostics.entries().len(), 1);
        assert!(prepare(&Diagnostics::new(true), None).is_err());
        Ok(())
    }
}
