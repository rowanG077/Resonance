//! Presentation inputs share resource handles with the prepared simulation.
use anyhow::{Context, Result, ensure};
use resonance_battle::ResourceId;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderInstances {
    Fixed(usize),
    /// Bone offsets for each visual layer in the particle's sampled pose.
    Particles(Vec<usize>),
}

pub struct RenderModel {
    pub resource: ResourceId,
    pub skeleton: Skeleton,
    pub parts: Vec<PreviewPart>,
    pub instances: RenderInstances,
    pub lit: bool,
    pub texture_channels: Vec<resonance_content::battle_profile::TextureChannel>,
    pub weapon_style: Option<resonance_content::battle_profile::WeaponStyle>,
    /// Hide only the named mesh, preserving its transform.
    pub suppressed_nodes: BTreeSet<usize>,
    pub target_anchor: Option<TargetAnchor>,
}

#[derive(Clone, Copy)]
pub struct TargetAnchor {
    pub bone: Option<u16>,
    pub offset: [f32; 3],
}

impl TargetAnchor {
    pub(super) fn prepare(
        anchor: &resonance_content::battle_profile::TargetAnchor,
        skeleton: &Skeleton,
    ) -> Self {
        Self {
            bone: (anchor.bone != 0).then_some(
                if usize::from(anchor.bone) < skeleton.bones.len() {
                    u16::from(anchor.bone)
                } else {
                    0
                },
            ),
            offset: anchor.offset,
        }
    }
}

/// Persistent attached artwork shares the ordinary sampled-model renderer.
pub struct ModelAccessory {
    pub owner: resonance_battle::ActorId,
    pub resource: ResourceId,
    pub attachment: Option<usize>,
    pub transform: resonance_content::animation::Transform,
    pub skeleton: Skeleton,
    pub motion: Option<resonance_content::animation::Motion>,
    pub style: AccessoryStyle,
    pub visible: bool,
    pub inherit_tint: bool,
    pub depth_write: bool,
}

pub enum AccessoryStyle {
    Colette {
        sparkle_texture: resonance_content::battle_effect::EffectTexture,
    },
    EnemyWings,
    Ordinary,
}

impl ModelAccessory {
    pub const SPARKLE_PALETTES: [u8; 5] = [27, 28, 29, 30, 31];

    pub fn sparkle_texture(&self) -> Option<&resonance_content::battle_effect::EffectTexture> {
        match &self.style {
            AccessoryStyle::Colette { sparkle_texture } => Some(sparkle_texture),
            AccessoryStyle::EnemyWings | AccessoryStyle::Ordinary => None,
        }
    }
}

pub(super) fn wings(
    files: &resonance_content::prepared::Files,
    source: &resonance_content::battle_model::Party,
    owner: resonance_battle::ActorId,
    resource: ResourceId,
    common: &SourceBank,
) -> Result<(ModelAccessory, RenderModel)> {
    let sparkle_texture = common
        .art
        .as_ref()
        .and_then(|art| art.textures.get(&0))
        .and_then(|textures| textures.first())
        .context("wing sparkle atlas is not prepared")?
        .clone();
    for palette in ModelAccessory::SPARKLE_PALETTES {
        let page = sparkle_texture.palette_image(0, palette)?;
        let image = &sparkle_texture.images[page];
        ensure!(
            image.width == 512 && image.height == 512,
            "invalid wing sparkle atlas dimensions"
        );
    }
    let scene = source
        .wings
        .as_ref()
        .context("Colette wings are not prepared; refresh party artwork")?;
    let bone = source
        .body
        .skeleton
        .bones
        .iter()
        .position(|bone| bone.name == resonance_events::Wings::ATTACHMENT_BONE)
        .context("wing attachment bone is missing")?;
    let [x, y] = resonance_events::Wings::FLIGHT_TILT;
    let binding = resonance_content::battle_model::AttachedModel {
        model_slot: 0,
        motion_slot: Some(resonance_events::animation::slot::IDLE as u8),
        placement: resonance_content::battle_model::AttachmentPlacement::Bone(bone.try_into()?),
        visible: true,
        additive: true,
        inherit_tint: false,
        wing_layers: true,
        transform: resonance_content::animation::Transform {
            rotation: resonance_content::animation::euler_rotation([x as f32, y as f32, 0.]),
            ..Default::default()
        },
    };
    prepare_accessory(
        files,
        &[PreviewPart {
            scene: scene.clone(),
            animation: None,
            attached_to: None,
            additive: true,
            uv_offsets: vec![],
        }],
        &source.body.skeleton,
        owner,
        resource,
        AccessoryStyle::Colette { sparkle_texture },
        &binding,
    )
}

pub(super) fn prepare_accessory(
    files: &resonance_content::prepared::Files,
    parts: &[PreviewPart],
    body: &Skeleton,
    owner: resonance_battle::ActorId,
    resource: ResourceId,
    style: AccessoryStyle,
    binding: &resonance_content::battle_model::AttachedModel,
) -> Result<(ModelAccessory, RenderModel)> {
    let scene = &parts.first().context("attached model has no mesh")?.scene;
    let skeleton = Skeleton::from_glb(&files.read(&scene.mesh)?)?;
    ensure!(
        skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(scene.bone_names.iter()),
        "attached mesh skeleton differs from its descriptor"
    );
    binding.transform.validate()?;
    let motion = binding
        .motion_slot
        .map(|slot| -> Result<_> {
            let clip = scene
                .clips
                .iter()
                .find(|clip| {
                    clip.animation_resource.is_none() && clip.resource_slot == u16::from(slot)
                })
                .context("attached model motion is missing")?;
            let motion = resonance_content::animation::Motion::decode(&files.read(&clip.motion)?)?;
            motion.validate(&skeleton)?;
            ensure!(
                motion.duration_frames > 0.,
                "attached model motion has no duration"
            );
            Ok(motion)
        })
        .transpose()?;
    ensure!(
        !binding.wing_layers || motion.is_some(),
        "layered wings require motion"
    );
    let attachment = match binding.placement {
        resonance_content::battle_model::AttachmentPlacement::Bone(bone) => {
            ensure!(
                usize::from(bone) < body.bones.len(),
                "attached model bone is missing"
            );
            Some(usize::from(bone))
        }
        resonance_content::battle_model::AttachmentPlacement::Independent => None,
    };
    let model = RenderModel {
        resource,
        skeleton: skeleton.clone(),
        parts: parts
            .iter()
            .cloned()
            .map(|mut part| {
                part.animation = None;
                part.attached_to = None;
                part.additive = binding.additive;
                part
            })
            .collect(),
        instances: RenderInstances::Fixed(if binding.wing_layers { 3 } else { 1 }),
        lit: !binding.wing_layers,
        texture_channels: vec![],
        weapon_style: None,
        suppressed_nodes: BTreeSet::new(),
        target_anchor: None,
    };
    Ok((
        ModelAccessory {
            owner,
            resource,
            attachment,
            transform: binding.transform,
            skeleton,
            motion,
            style,
            visible: binding.visible,
            inherit_tint: binding.inherit_tint,
            depth_write: !binding.additive,
        },
        model,
    ))
}

/// Select a persistent body-atlas row without replacing textures or expression channels.
pub(super) fn enemy_body_parts(
    parts: &[PreviewPart],
    channel: Option<resonance_content::battle_profile::TextureChannel>,
    row: u8,
) -> Result<Vec<PreviewPart>> {
    let mut parts = parts
        .iter()
        .filter(|part| part.attached_to.is_none())
        .cloned()
        .collect::<Vec<_>>();
    let Some(channel) = channel else {
        ensure!(row == 0, "enemy has no prepared body atlas variants");
        return Ok(parts);
    };
    ensure!(
        row < channel.frames,
        "enemy body atlas row exceeds its frame count"
    );
    let primary = parts
        .first_mut()
        .context("enemy has no primary body artwork")?;
    ensure!(
        usize::from(channel.texture) < primary.scene.textures.len(),
        "enemy body atlas texture is absent"
    );
    primary
        .uv_offsets
        .resize(primary.scene.materials.len(), [0.; 4]);
    let y = f32::from(row) / f32::from(channel.frames);
    for (offset, material) in primary.uv_offsets.iter_mut().zip(&primary.scene.materials) {
        for (stage, binding) in [&material.color, &material.multiply]
            .into_iter()
            .enumerate()
        {
            if binding
                .as_ref()
                .is_some_and(|binding| binding.texture == usize::from(channel.texture))
            {
                offset[stage * 2 + 1] += y;
            }
        }
    }
    Ok(parts)
}

/// Publish the sampled model and its drawing resources together, or omit this effect.
pub(super) fn effect_model(
    files: &resonance_content::prepared::Files,
    part: &resonance_content::battle_model::ModelPart,
    resource: ResourceId,
    animate_layers: bool,
) -> Result<Option<(resonance_battle::PreparedEffectModel, RenderModel)>> {
    let prepared =
        crate::battle::model::effect(files, part, resource, animate_layers).map(|model| {
            let mut next_bone = part.rig.skeleton.bones.len();
            let bone_offsets = part
                .layers
                .iter()
                .map(|layer| {
                    if let Some(skeleton) = part.layer_skeletons.get(&layer.scene.resource) {
                        let offset = next_bone;
                        next_bone += skeleton.bones.len();
                        offset
                    } else {
                        0
                    }
                })
                .collect();
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
                    instances: RenderInstances::Particles(bone_offsets),
                    lit: true,
                    texture_channels: vec![],
                    suppressed_nodes: BTreeSet::new(),
                    target_anchor: None,
                    weapon_style: None,
                },
            )
        });
    files.diagnostics().attempt("battle effect model", prepared)
}

/// Stored spells and physical skills share the same prepared model/effect bank.
pub(super) fn scene_bank(
    files: &resonance_content::prepared::Files,
    bank: &Arc<SourceBank>,
    resource: ResourceId,
    mut next_resource: impl FnMut() -> ResourceId,
    models: &mut Vec<RenderModel>,
) -> Result<crate::battle::EffectResource> {
    let mut prepared = BTreeMap::new();
    for (&slot, part) in bank.art.iter().flat_map(|art| &art.models) {
        if let Some((model, mut render)) = effect_model(files, part, next_resource(), false)? {
            render.suppressed_nodes = suppressed_body_nodes(&part.rig.skeleton, false);
            prepared.insert(slot, model);
            models.push(render);
        }
    }
    Ok(crate::battle::EffectResource {
        bank: Some(Arc::clone(bank)),
        resource,
        members: Vec::new(),
        models: prepared,
    })
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
    pub resource: ResourceId,
    pub source: Arc<SourceBank>,
    pub textures: BTreeMap<u8, Vec<resonance_content::battle_effect::EffectTexture>>,
    /// Particle declarations reached by the selected programs.
    pub members: BTreeSet<u16>,
    pub palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
    pub blends: BTreeMap<u16, BTreeSet<u8>>,
}

pub struct RenderTrail {
    /// Primary weapon layer that supplies the live pose.
    pub resource: ResourceId,
    pub bones: Vec<u16>,
    pub material: TrailMaterial,
    /// Default material, then the highlighted material for party weapons.
    pub textures: Vec<Option<(UiTexture, Sampler)>>,
}

pub(super) fn effects(
    resource: ResourceId,
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
                if birth.palette.is_some() || birth.alpha_palette.is_some() || palette.is_some() {
                    let initial = source.particle(usize::from(*particle))?.state.palettes;
                    let choices = |range: Option<resonance_content::battle_effect::ValueRange>,
                                   fallback| {
                        range.map_or(fallback..=fallback, |range| {
                            range.min.round().clamp(0., 255.) as u8
                                ..=range.max.round().clamp(0., 255.) as u8
                        })
                    };
                    for color in choices(birth.palette, palette.unwrap_or(initial[0])) {
                        for alpha in choices(birth.alpha_palette, initial[1]) {
                            palettes.entry(member).or_default().insert([color, alpha]);
                        }
                    }
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
    resource: ResourceId,
    material: TrailMaterial,
    party_weapon: bool,
    common: Option<&SourceBank>,
    enemy: Option<&SourceBank>,
) -> Result<Option<RenderTrail>> {
    let prepared = (|| {
        let Some(bones) = crate::battle::trail::weapon_endpoints(skeleton)? else {
            return Ok(None);
        };
        let mut palettes = vec![material.palette];
        if party_weapon {
            palettes.push(1);
        }
        let textures = if let Some(texture) = material.texture {
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
            palettes
                .into_iter()
                .map(|palette| {
                    let page = image.palette_image(0, palette)?;
                    Ok(Some((image.images[page].clone(), image.sampler.clone())))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![None; palettes.len()]
        };
        Ok(Some(RenderTrail {
            resource,
            bones,
            material,
            textures,
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
    fn enemy_atlas_rows_offset_only_the_primary_body_and_reject_invalid_rows() -> Result<()> {
        use resonance_content::battle_profile::TextureChannel;
        let part: PreviewPart = serde_json::from_value(serde_json::json!({
            "scene": {
                "resource": 0, "mesh": "body.glb", "textures": ["face", "body"],
                "materials": [{
                    "color": {"texture": 1, "wrap_u": "repeat", "wrap_v": "repeat", "nearest_min": false, "nearest_mag": false},
                    "multiply": {"texture": 0, "wrap_u": "repeat", "wrap_v": "repeat", "nearest_min": false, "nearest_mag": false},
                    "vertex_color": true, "blend": false, "depth_write": true, "draw_order": 0
                }],
                "translation": [0.,0.,0.], "clips": [], "autoplay": false, "texture_animations": []
            },
            "animation": null, "attached_to": null, "additive": false,
            "uv_offsets": [[0.125, 0., 0.25, 0.5]]
        }))?;
        let parts = [part.clone(), part];
        let channel = Some(TextureChannel {
            texture: 1,
            frames: 4,
        });
        let selected = enemy_body_parts(&parts, channel, 1)?;
        assert_eq!(selected[0].uv_offsets, [[0.125, 0.25, 0.25, 0.5]]);
        assert_eq!(selected[1].uv_offsets, parts[1].uv_offsets);
        assert_eq!(parts[0].uv_offsets, [[0.125, 0., 0.25, 0.5]]);
        assert!(enemy_body_parts(&parts, channel, 4).is_err());
        assert!(
            enemy_body_parts(
                &parts,
                Some(TextureChannel {
                    texture: 1,
                    frames: 0
                }),
                0
            )
            .is_err()
        );
        assert!(
            enemy_body_parts(
                &parts,
                Some(TextureChannel {
                    texture: 2,
                    frames: 4
                }),
                1
            )
            .is_err()
        );
        assert!(enemy_body_parts(&parts, None, 1).is_err());
        Ok(())
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
        let mut files = Files::new(Diagnostics::default());
        let mut node = serde_json::to_value(Transform::default())?;
        node["name"] = "root".into();
        node["extras"] = serde_json::json!({"resonance_bone": 0});
        let mut json = serde_json::to_vec(&serde_json::json!({
            "asset": {"version": "2.0"}, "nodes": [node],
        }))?;
        json.resize(json.len().next_multiple_of(4), b' ');
        let mut mesh = b"glTF".to_vec();
        for word in [2_u32, 20 + json.len() as u32, json.len() as u32] {
            mesh.extend(word.to_le_bytes());
        }
        mesh.extend(b"JSON");
        mesh.extend(json);
        files.insert("effect.glb".into(), mesh.into());
        let (model, render) = effect_model(&files, &part, ResourceId(1), false)?.unwrap();
        assert_eq!(model.resource(), render.resource);
        part.layers[0].scene.bone_names.clear();
        assert!(effect_model(&files, &part, ResourceId(2), false)?.is_none());
        assert_eq!(files.diagnostics().entries().len(), 1);
        assert!(effect_model(&Files::default(), &part, ResourceId(2), false).is_err());
        part.layers[0].scene.bone_names.push("root".into());
        assert!(effect_model(&files, &part, ResourceId(3), false)?.is_some());
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
        let prepared = effects(ResourceId(77), &source, &[0, 1], None, &diagnostics).unwrap();
        assert!(prepared.members.is_empty());
        assert!(prepared.textures.is_empty());
        assert_eq!(diagnostics.entries().len(), 1);
        assert!(
            effects(
                ResourceId(77),
                &source,
                &[0, 1],
                None,
                &Diagnostics::new(true)
            )
            .is_err()
        );
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
        // A tint-capable particle still uses its authored palette when the
        // effect is spawned without elemental feedback.
        if let resonance_content::battle_effect::declaration::Declaration::Particle {
            template,
            ..
        } = &mut source.actors[0]
        {
            template.element_tint = true;
        }
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
        let result = effects(ResourceId(77), &source, &[0], None, &Diagnostics::new(true)).unwrap();
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
            weapon_trail(
                diagnostics,
                &skeleton,
                ResourceId(1),
                material,
                false,
                Some(&common),
                enemy,
            )
        };
        let render = prepare(&diagnostics, Some(&enemy))?.unwrap();
        assert_eq!(render.textures[0].as_ref().unwrap().0.path, "owner-1.ktx2");
        assert!(prepare(&diagnostics, None)?.is_none());
        assert_eq!(diagnostics.entries().len(), 1);
        assert!(prepare(&Diagnostics::new(true), None).is_err());
        Ok(())
    }
}
