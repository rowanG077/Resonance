//! Enemy models from native battle packages, sharing identical motion files.
use crate::{
    battle_equipment, dol, enemy_package::EnemyPackages, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace, SceneClip, ScenePart, TextureAnimation,
    animation::{Motion, Skeleton},
    battle_model::document::{Model, enemy::Variant},
    model_behavior::ModelBehaviorBinding,
    model_preview::{ModelPreview, PreviewPart},
    secondary_motion::{
        ChainDefinition, Definition as SecondaryMotion, Parameter, PlaneDefinition,
    },
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::MonsterId;

#[derive(Deserialize)]
struct PreviewSelector {
    #[serde(default)]
    monsters: Vec<MonsterId>,
}

#[derive(Deserialize)]
struct PreviewRule {
    select: PreviewSelector,
    module: String,
    function: String,
}

fn preview_behavior(id: MonsterId) -> Result<Option<ModelBehaviorBinding>> {
    let rules: Vec<PreviewRule> =
        serde_json::from_str(include_str!("../../../scripts/preview/bindings.json"))?;
    let mut selected = rules
        .into_iter()
        .filter(|rule| rule.select.monsters.contains(&id));
    let behavior = selected.next().map(|rule| ModelBehaviorBinding {
        module: rule.module,
        function: rule.function,
    });
    ensure!(
        selected.next().is_none(),
        "duplicate enemy preview behavior"
    );
    Ok(behavior)
}

pub(crate) fn member<'a>(package: &'a [u8], field: usize) -> Result<Option<&'a [u8]>> {
    let start = dol::u32(package, field)? as usize;
    if start == 0 {
        return Ok(None);
    }
    ensure!(
        start >= 0x1e8 && start < package.len(),
        "invalid enemy member"
    );
    let mut end = package.len();
    for at in (0x18..0x1e8).step_by(4) {
        let other = dol::u32(package, at)? as usize;
        if other > start && other < end {
            end = other;
        }
    }
    Ok(Some(&package[start..end]))
}

fn palette(model: &[u8]) -> Result<&[u8]> {
    let start = dol::u32(model, 0)? as usize;
    let end = dol::u32(model, 4)? as usize;
    let data = model
        .get(start..end)
        .context("enemy palette exceeds model")?;
    ensure!(dol::u32(data, 0)? == 0x0020_af30, "missing enemy TPL");
    Ok(data)
}

fn clip(path: String, slot: u16) -> SceneClip {
    SceneClip {
        motion: path,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: None,
        pose_chains: Vec::new(),
    }
}

fn duplicate_motion_joints(bytes: &[u8]) -> Result<Option<usize>> {
    let count = usize::from(dol::u16(bytes, 12)?);
    let mut joints = BTreeMap::new();
    for index in 0..count {
        let node = dol::u16(bytes, 36 + index * 16 + 10)?;
        joints.entry(node).or_insert(index);
    }
    if joints.len() == count {
        return Ok(None);
    }
    let mut cursor = dol::u32(bytes, 20)? as usize;
    if cursor != 0 {
        let mut names = Vec::new();
        for _ in 0..count {
            let tail = bytes
                .get(cursor..)
                .context("enemy motion name table is truncated")?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .context("enemy motion name is unterminated")?;
            names.push(&tail[..end]);
            cursor += end + 1;
        }
        for index in joints.values() {
            let node = dol::u16(bytes, 36 + index * 16 + 10)?;
            for other in 0..count {
                if other != *index && dol::u16(bytes, 36 + other * 16 + 10)? == node {
                    ensure!(
                        names[*index] == names[other],
                        "duplicate enemy joint has different names"
                    );
                }
            }
        }
    }
    Ok(Some(joints.len()))
}

fn enemy_blend(scene: &mut ScenePart, alpha: &[bool]) {
    for material in &mut scene.materials {
        material.blend |= material
            .color
            .as_ref()
            .is_some_and(|binding| alpha.get(binding.texture).copied().unwrap_or(false))
            || material.multiply.is_some();
    }
}

fn animate_body(parts: &mut [PreviewPart], metadata: &[u8]) -> Result<()> {
    let settings = metadata
        .get(0xfa..0x102)
        .context("missing enemy texture animation settings")?;
    let mut animations = Vec::new();
    for channel in 0..2 {
        let texture = usize::from(settings[2 + channel]);
        let amount = i32::from(settings[4 + channel] as i8);
        let rate = i32::from(settings[6 + channel] as i8);
        let (delay_ticks, offsets) = match settings[channel] {
            0 => continue,
            1 => {
                ensure!(amount > 0 && rate >= 0, "invalid enemy texture atlas");
                let interval = rate as usize + 1;
                let frames = amount as usize;
                (
                    1,
                    (0..frames * interval)
                        .map(|tick| [0., (tick / interval) as f32 / frames as f32])
                        .collect(),
                )
            }
            2 => {
                let mut phase = [0i32; 2];
                let mut offsets = Vec::new();
                loop {
                    offsets.push(phase.map(|value| value as f32 / 1000.));
                    phase[0] = (phase[0] + amount).rem_euclid(1000);
                    phase[1] = (phase[1] + rate).rem_euclid(1000);
                    if phase == [0; 2] {
                        break;
                    }
                }
                (0, offsets)
            }
            mode => anyhow::bail!("unsupported enemy texture animation mode {mode}"),
        };
        animations.push(TextureAnimation {
            texture,
            delay_ticks,
            loop_start: 0,
            offsets,
        });
    }
    for part in parts {
        part.scene.texture_animations = animations
            .iter()
            .filter(|animation| {
                part.scene.materials.iter().any(|material| {
                    [&material.color, &material.multiply]
                        .into_iter()
                        .flatten()
                        .any(|binding| binding.texture == animation.texture)
                })
            })
            .cloned()
            .collect();
    }
    Ok(())
}

fn chain_parameter(name: &str, tag: &str) -> Result<Option<f32>> {
    let Some((_, value)) = name.split_once(tag) else {
        return Ok(None);
    };
    let encoded = value
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '+' | 'e' | '.'))
        .map(|ch| if ch == 'e' { '.' } else { ch })
        .collect::<String>();
    let value: f32 = encoded.parse()?;
    ensure!(
        value.is_finite(),
        "invalid enemy secondary motion parameter"
    );
    Ok((value != 0.).then_some(value))
}

fn chain_name(source: &str, used: &mut BTreeSet<String>) -> String {
    let stem = source
        .split('_')
        .take_while(|word| !["Pow", "Bb", "Dt"].iter().any(|tag| word.starts_with(tag)))
        .filter(|word| !matches!(*word, "" | "AB" | "ROOT" | "ROOT01" | "FP" | "NR" | "01"))
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .replace("rkami", "kami-right")
        .replace("lkami", "kami-left");
    let mut words = Vec::new();
    for part in stem.split('-') {
        let digits = part
            .find(|ch: char| ch.is_ascii_digit())
            .unwrap_or(part.len());
        let (word, number) = part.split_at(digits);
        let label = match word {
            "kami" | "hair" => "hair",
            "manto" | "mant" => "cape",
            "nuno" => "cloth",
            "kata" => "shoulder-cloth",
            "koshi" | "kosi" => "waist-cloth",
            "osage" => "braid",
            "momi" => "sideburns",
            "hige" => "beard",
            "mayuge" => "eyebrows",
            "mimi" => "ear",
            "sippo" => "tail",
            "hachimaki" => "headband",
            "fbelt" => "front-belt",
            "fusa" => "tassel",
            "non" => "unidentified",
            "l" => "left",
            "r" => "right",
            "b" => "back",
            "f" => "front",
            other => other,
        };
        if matches!(word, "hane" | "nemoto" | "kzl" | "kzr") {
            words.push("unidentified".to_owned());
        }
        if !label.is_empty() {
            words.push(label.to_owned());
        }
        if !number.is_empty() {
            words.push(
                number
                    .parse::<u32>()
                    .map_or_else(|_| number.to_owned(), |n| n.to_string()),
            );
        }
    }
    let stem = if words.is_empty() {
        "unidentified".to_owned()
    } else {
        words.join("-")
    };
    let mut label = stem.clone();
    let mut variant = 2;
    while !used.insert(label.clone()) {
        label = format!("{stem}-{variant}");
        variant += 1;
    }
    label
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChainTuning {
    models: Vec<String>,
    chains: Vec<String>,
    attraction: f32,
    gravity: f32,
    damping: f32,
    #[serde(default)]
    taper: bool,
    #[serde(default)]
    weighted_root: bool,
    #[serde(default)]
    lock_y: bool,
    collision_plane: Option<PlaneDefinition>,
}

fn tune_chains(model: &str, chains: &mut [ChainDefinition]) -> Result<()> {
    let rules: Vec<ChainTuning> =
        serde_json::from_str(include_str!("enemy-secondary-tuning.json"))?;
    let mut tuned = BTreeSet::new();
    for rule in rules
        .into_iter()
        .filter(|rule| rule.models.iter().any(|name| name == model))
    {
        for name in &rule.chains {
            let chain = chains
                .iter_mut()
                .find(|chain| &chain.name == name)
                .with_context(|| format!("missing authored enemy chain {model}/{name}"))?;
            ensure!(tuned.insert(name.clone()), "duplicate enemy chain tuning");
            let mut gravity = rule.gravity;
            let mut damping = rule.damping;
            if rule.weighted_root {
                gravity *= 1.66;
                damping *= 1.66;
            }
            let mut gravities = Vec::new();
            let mut dampings = Vec::new();
            for _ in &chain.joints {
                gravities.push(gravity);
                dampings.push(damping);
                if rule.taper {
                    gravity /= 1.2;
                    damping /= 1.2;
                }
            }
            chain.attraction = rule.attraction;
            chain.gravity = if rule.taper {
                Parameter::PerJoint(gravities)
            } else {
                Parameter::Uniform(gravities[0])
            };
            chain.damping = if rule.taper {
                Parameter::PerJoint(dampings)
            } else {
                Parameter::Uniform(dampings[0])
            };
            chain.lock_rotation_y = rule.lock_y;
            chain.collision_plane = rule.collision_plane.clone();
        }
    }
    Ok(())
}

fn posed_chains(
    motion: &[u8],
    bindings: &[(u16, String, String)],
    dynamics: &SecondaryMotion,
) -> Result<Vec<String>> {
    // The native name table is optional and sometimes malformed; the motion
    // encoder itself falls back to binding IDs in that case.
    let names = world_motion::shared_track_names(motion, bindings).unwrap_or_else(|_| {
        bindings
            .iter()
            .map(|(id, name, _)| (*id, name.clone()))
            .collect()
    });
    let count = usize::from(dol::u16(motion, 12)?);
    let mut posed = BTreeSet::new();
    for index in 0..count {
        let at = 36 + index * 16;
        if dol::u16(motion, at + 8)? > 2
            && let Some(name) = names.get(&dol::u16(motion, at + 10)?)
        {
            posed.insert(name.as_str());
        }
    }
    let mut selected = dynamics
        .chains
        .iter()
        .filter(|chain| {
            chain
                .joints
                .first()
                .is_some_and(|joint| posed.contains(joint.as_str()))
        })
        .map(|chain| chain.name.clone())
        .collect::<Vec<_>>();
    selected.sort();
    Ok(selected)
}

fn secondary_chains(
    glb: &[u8],
    bindings: &[(u16, String, String)],
    model: &str,
) -> Result<SecondaryMotion> {
    let skeleton = Skeleton::from_glb(glb)?;
    ensure!(
        skeleton.bones.len() == bindings.len(),
        "enemy bone binding count differs"
    );
    let mut children = vec![Vec::new(); skeleton.bones.len()];
    for (index, bone) in skeleton.bones.iter().enumerate() {
        if let Some(parent) = bone.parent {
            children[usize::from(parent)].push(index);
        }
    }
    let mut used = BTreeSet::new();
    let mut chains = Vec::new();
    for (root, (_, _, source)) in bindings.iter().enumerate() {
        if !source.starts_with("AB_ROOT") {
            continue;
        }
        let mut joints = Vec::new();
        let mut gravity = Vec::new();
        let mut damping = Vec::new();
        let mut node = root;
        let root_gravity = chain_parameter(source, "_Dt")?.unwrap_or(1.2);
        let root_damping = chain_parameter(source, "_Bb")?.unwrap_or(0.1);
        let mut attraction = 0.019;
        let mut follows_pose = false;
        let mut preserve_rotation = false;
        loop {
            ensure!(joints.len() < 128, "enemy secondary chain is oversized");
            let raw = &bindings[node].2;
            joints.push(skeleton.bones[node].name.clone());
            gravity.push(chain_parameter(raw, "_Dt")?.unwrap_or(root_gravity));
            damping.push(chain_parameter(raw, "_Bb")?.unwrap_or(root_damping));
            attraction = chain_parameter(raw, "_Pow")?.unwrap_or(attraction);
            follows_pose |= raw.contains("_FP_");
            preserve_rotation |= raw.contains("_NR_");
            match children[node].as_slice() {
                [] => break,
                [next] => node = *next,
                _ => anyhow::bail!("enemy secondary chain branches"),
            }
        }
        let parameter = |values: Vec<f32>| {
            if values.iter().all(|value| *value == values[0]) {
                Parameter::Uniform(values[0])
            } else {
                Parameter::PerJoint(values)
            }
        };
        chains.push(ChainDefinition {
            name: chain_name(source, &mut used),
            joints,
            gravity: parameter(gravity),
            damping: parameter(damping),
            attraction: if follows_pose { attraction } else { 0. },
            pose_attraction: 0.2,
            preserve_rotation,
            lock_rotation_x: false,
            lock_rotation_y: false,
            collision_plane: None,
        });
    }
    tune_chains(model, &mut chains)?;
    let definition = SecondaryMotion { chains };
    definition.prepare(
        &skeleton
            .bones
            .iter()
            .map(|bone| bone.name.clone())
            .collect::<Vec<_>>(),
    )?;
    Ok(definition)
}

fn nested_section(package: &[u8], index: usize) -> Result<Option<&[u8]>> {
    let count = dol::u32(package, 0)? as usize;
    ensure!(
        (5..=6).contains(&count) && index < count,
        "unsupported carried model package"
    );
    let start = dol::u32(package, 4 + index * 4)? as usize;
    if start == 0 {
        return Ok(None);
    }
    let end = (0..count)
        .filter_map(|other| dol::u32(package, 4 + other * 4).ok())
        .map(|offset| offset as usize)
        .filter(|&offset| offset > start)
        .min()
        .unwrap_or(package.len());
    ensure!(
        start >= 4 + count * 4 && start < end,
        "invalid carried model section"
    );
    Ok(Some(
        package
            .get(start..end)
            .context("carried model section exceeds package")?,
    ))
}

fn carried_pose_clips(
    package: &[u8],
    attachments: usize,
) -> Result<BTreeMap<usize, BTreeSet<u16>>> {
    let start = usize::from(dol::u16(package, 14)?);
    let end = usize::from(dol::u16(package, 16)?);
    let commands = package
        .get(start..end)
        .context("enemy carried pose commands exceed package")?;
    let mut clips = BTreeMap::<usize, BTreeSet<u16>>::new();
    for at in (0..commands.len().saturating_sub(15)).step_by(2) {
        if dol::u16(commands, at + 2)? != 25 {
            continue;
        }
        let slot = usize::from(dol::u16(commands, at + 4)?);
        let clip = dol::u16(commands, at + 6)?;
        let rate = f32::from_bits(dol::u32(commands, at + 8)?);
        let repeat = dol::u32(commands, at + 12)?;
        let field = 0x20 + usize::from(clip) * 4;
        if slot < attachments
            && field < 0x160
            && dol::u32(package, field)? != 0
            && rate.is_finite()
            && rate > 0.
            && repeat <= 1
        {
            clips.entry(slot).or_default().insert(clip);
        }
    }
    Ok(clips)
}

fn carried_attachment(
    package: &[u8],
    metadata: &[u8],
    body: &[u8],
    name: &str,
    base: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    motions: &mut BTreeMap<Vec<u8>, String>,
    bones: &[String],
    slot: usize,
    resource: u16,
    additional: bool,
    pose_clips: Option<&BTreeSet<u16>>,
) -> Result<PreviewPart> {
    let carried = member(package, 0x160 + slot * 4)?.context("enemy carried model is missing")?;
    ensure!(
        nested_section(carried, 0)?.is_some()
            && nested_section(carried, 2)?.is_none()
            && nested_section(carried, 4)?.is_none(),
        "enemy has layered carried model"
    );
    let model = nested_section(carried, if additional { 5 } else { 1 })?
        .context("enemy carried geometry is missing")?;
    let owned_palette = dol::u32(model, 0)? != 0;
    let source = if owned_palette {
        model.to_vec()
    } else {
        world_gpl::with_primary_palette(body, model)?
    };
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    let tpl = palette(&source)?;
    for index in 0..dol::u32(tpl, 4)? as usize {
        let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
        let path = if let Some((path, _)) = files
            .iter()
            .find(|(path, old)| path.starts_with(&format!("{base}/textures/")) && **old == texture)
        {
            path.clone()
        } else {
            let path = format!("{base}/textures/part-{resource}-texture-{index}.ktx2");
            files.insert(path.clone(), texture);
            path
        };
        paths.push(path);
        alpha.push(transparent);
    }
    let (glb, mut scene) = world_gpl::static_model(&source, paths, alpha.clone())?;
    enemy_blend(&mut scene, &alpha);
    if scene.materials.is_empty() {
        scene.textures.clear();
    }
    scene.mesh = if let Some((path, _)) = files
        .iter()
        .find(|(path, old)| path.starts_with(&format!("{base}/meshes/part-")) && **old == glb)
    {
        path.clone()
    } else {
        let path = format!("{base}/meshes/part-{resource}.glb");
        files.insert(path.clone(), glb.clone());
        path
    };
    if !additional && let Some(motion) = nested_section(carried, 3)? {
        let bindings = world_gpl::named_animation_bindings(&source)?;
        let native = bindings
            .into_iter()
            .map(|(id, _, raw)| (id, raw))
            .collect::<Vec<_>>();
        let encoded = world_motion::encode_bound(motion, &glb, &native)
            .with_context(|| format!("enemy {name} carried motion {slot}"))?;
        ensure!(
            !Motion::decode(&encoded)?.tracks.is_empty(),
            "enemy carried motion has no bound tracks"
        );
        let path = if let Some(path) = motions.get(&encoded) {
            path.clone()
        } else {
            let path = format!("battle/enemies/{name}/attachment-{slot}-layer-0-animation-0.glb");
            motions.insert(encoded.clone(), path.clone());
            ensure!(
                files.insert(path.clone(), encoded).is_none(),
                "duplicate carried motion"
            );
            path
        };
        scene.clips.push(clip(path, 0));
    }
    if !additional && let Some(pose_clips) = pose_clips {
        let bindings = world_gpl::named_animation_bindings(&source)?
            .into_iter()
            .map(|(id, _, raw)| (id, raw))
            .collect::<Vec<_>>();
        for &slot_id in pose_clips {
            let motion = member(package, 0x20 + usize::from(slot_id) * 4)?
                .context("enemy carried pose has no native motion")?;
            ensure!(
                dol::u32(motion, 0)? == 0x007b_7960,
                "invalid carried pose motion"
            );
            let encoded = world_motion::encode_bound(motion, &glb, &bindings)
                .with_context(|| format!("enemy {name} carried pose {slot_id}"))?;
            ensure!(
                !Motion::decode(&encoded)?.tracks.is_empty(),
                "enemy carried pose has no bound tracks"
            );
            let path =
                format!("battle/enemies/{name}/attachment-{slot}-layer-0-animation-{slot_id}.glb");
            ensure!(
                files.insert(path.clone(), encoded).is_none(),
                "duplicate carried pose"
            );
            scene.clips.push(clip(path, slot_id));
        }
    }
    scene.resource = resource;
    let additive = additional || metadata[0x124 + slot * 24] & 0x10 != 0;
    for material in &mut scene.materials {
        material.draw_order += u32::from(resource) * resonance_content::field::MODEL_DRAW_SPAN;
        if additive {
            material.cull = CullFace::None;
            material.blend = true;
            material.depth_write = false;
        }
    }
    let bone = bones
        .iter()
        .find(|bone| bone.starts_with(&format!("kk0{slot}")))
        .with_context(|| format!("enemy carried model has no kk0{slot} attachment joint"))?;
    Ok(PreviewPart {
        scene,
        animation: None,
        attached_to: Some(bone.clone()),
        additive,
        uv_offsets: Vec::new(),
    })
}

fn matching_equipment_file(library: &Path, file: &str, bytes: &[u8]) -> Result<Option<String>> {
    let directory = library.join("meshes/equipment");
    if !directory.exists() {
        return Ok(None);
    }
    let mut names = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        let path = Path::new("meshes/equipment").join(name).join(file);
        let full = library.join(&path);
        if full.is_file() && fs::read(full)? == bytes {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

fn matching_equipment_texture(library: &Path, mesh: &str, bytes: &[u8]) -> Result<Option<String>> {
    let Some(owner) = mesh.strip_prefix("meshes/equipment/") else {
        return Ok(None);
    };
    let owner = owner
        .split('/')
        .next()
        .context("equipment mesh has no owner")?;
    let directory = Path::new("meshes/equipment").join(owner);
    let mut names = fs::read_dir(library.join(&directory))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        if !name
            .to_string_lossy()
            .starts_with("part-0-layer-0-texture-")
        {
            continue;
        }
        let path = directory.join(name);
        if fs::read(library.join(&path))? == bytes {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

fn variant_models(
    package: &[u8],
    name: &str,
    library: &Path,
) -> Result<(BTreeMap<u8, Variant>, BTreeMap<String, Vec<u8>>)> {
    let profile = package
        .get(usize::from(dol::u16(package, 4)?)..)
        .context("enemy profile exceeds package")?;
    let equipped = usize::from(
        *profile
            .get(0x1e4)
            .context("missing enemy attachment count")?,
    );
    ensure!(equipped <= 8, "enemy has more than eight carried models");
    let slots = (equipped..8)
        .filter(|slot| dol::u32(package, 0x160 + slot * 4).is_ok_and(|offset| offset != 0))
        .collect::<Vec<_>>();
    if slots.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }
    ensure!(equipped != 0, "weapon variant has no equipped attachment");
    let body = member(package, 0x18)?.context("enemy body is missing")?;
    let mut variants = BTreeMap::new();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for slot in slots {
        let carried = member(package, 0x160 + slot * 4)?.context("weapon variant is missing")?;
        ensure!(
            nested_section(carried, 2)?.is_none() && nested_section(carried, 4)?.is_none(),
            "layered weapon variant needs a decoder"
        );
        let settings =
            nested_section(carried, 0)?.context("weapon variant settings are missing")?;
        let model = nested_section(carried, 1)?.context("weapon variant geometry is missing")?;
        let source = if dol::u32(model, 0)? != 0 {
            model.to_vec()
        } else {
            world_gpl::with_primary_palette(body, model)?
        };
        let stem = format!("battle/enemies/{name}/weapon-variant-{slot}-layer-0");
        let tpl = palette(&source)?;
        let mut images = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..dol::u32(tpl, 4)? as usize {
            let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
            images.push(texture);
            alpha.push(transparent);
        }
        let provisional = (0..images.len())
            .map(|index| format!("{stem}-texture-{index}.ktx2"))
            .collect();
        let (glb, mut scene) = world_gpl::static_model(&source, provisional, alpha.clone())?;
        enemy_blend(&mut scene, &alpha);
        scene.mesh =
            if let Some(path) = matching_equipment_file(library, "part-0-layer-0.glb", &glb)? {
                path
            } else {
                let path = format!("{stem}.glb");
                ensure!(
                    files.insert(path.clone(), glb.clone()).is_none(),
                    "duplicate variant mesh"
                );
                path
            };
        scene.textures = Vec::new();
        for (index, texture) in images.into_iter().enumerate() {
            let path =
                if let Some(path) = matching_equipment_texture(library, &scene.mesh, &texture)? {
                    path
                } else if let Some((path, _)) = files
                    .iter()
                    .find(|(path, bytes)| path.ends_with(".ktx2") && **bytes == texture)
                {
                    path.clone()
                } else {
                    let path = format!("{stem}-texture-{index}.ktx2");
                    ensure!(
                        files.insert(path.clone(), texture).is_none(),
                        "duplicate variant texture"
                    );
                    path
                };
            scene.textures.push(path);
        }
        let trail = battle_equipment::trail(&glb, settings)?;
        ensure!(trail.is_some(), "weapon variant has no trail endpoints");
        let animation = battle_equipment::texture_animation(settings)?;
        let mut selected = None;
        if let Some(motion) = nested_section(carried, 3)? {
            let native = world_gpl::named_animation_bindings(&source)?
                .into_iter()
                .map(|(id, _, raw)| (id, raw))
                .collect::<Vec<_>>();
            let encoded = world_motion::encode_bound(motion, &glb, &native)?;
            ensure!(
                !Motion::decode(&encoded)?.tracks.is_empty(),
                "weapon variant motion has no bound tracks"
            );
            let path = if let Some(path) =
                matching_equipment_file(library, "part-0-layer-0-animation-0.glb", &encoded)?
            {
                path
            } else {
                let path = format!("{stem}-animation-0.glb");
                ensure!(
                    files.insert(path.clone(), encoded).is_none(),
                    "duplicate variant motion"
                );
                path
            };
            scene.clips.push(clip(path, 0));
            scene.autoplay = true;
            selected = Some(0);
        }
        let part = PreviewPart {
            scene,
            animation: selected,
            attached_to: None,
            additive: profile[0x124] & 0x10 != 0,
            uv_offsets: Vec::new(),
        };
        let model = Model {
            attachment_nodes: BTreeMap::new(),
            independent_layers: BTreeSet::new(),
            layers: vec![part],
        };
        ensure!(
            variants
                .insert(
                    slot as u8,
                    Variant {
                        model,
                        trail,
                        animation
                    }
                )
                .is_none(),
            "duplicate weapon variant"
        );
    }
    Ok((variants, files))
}

/// Final alternate weapon artwork, sharing its native decoder with the media cook.
pub(crate) fn variant_artwork(
    package: &[u8],
    library: &Path,
    name: &str,
) -> Result<BTreeMap<u8, Variant>> {
    let (variants, files) = variant_models(package, name, library)?;
    ensure!(
        files.keys().all(|path| library.join(path).exists()),
        "weapon variant media is missing"
    );
    Ok(variants)
}

fn convert(
    packages: &mut EnemyPackages,
    id: u8,
    motions: &mut BTreeMap<Vec<u8>, String>,
    library: &Path,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let monster = MonsterId::try_from(id)?;
    let name = monster.resource_name().context("enemy has no name")?;
    let package = packages.read(id)?;
    ensure!(package.starts_with(b"em8\0"), "invalid enemy package");
    let body = member(&package, 0x18)?.context("enemy body is missing")?;
    let outline = member(&package, 0x1c)?;
    let metadata = package
        .get(usize::from(dol::u16(&package, 4)?)..)
        .context("enemy metadata exceeds package")?;
    ensure!(metadata.len() >= 0x1e8, "truncated enemy metadata");
    let attachments = usize::from(metadata[0x1e4]);
    let secondary_flags = dol::u16(metadata, 0xb4)? & 0x8400;
    ensure!(attachments <= 8, "enemy has more than eight carried models");
    let base = format!("meshes/monsters/{name}");
    let mut files = BTreeMap::new();
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    let tpl = palette(body)?;
    for index in 0..dol::u32(tpl, 4)? as usize {
        let path = format!("{base}/textures/body-texture-{index}.ktx2");
        let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
        files.insert(path.clone(), texture);
        paths.push(path);
        alpha.push(transparent);
    }
    let (glb, mut body_part) = world_gpl::static_model(body, paths.clone(), alpha.clone())?;
    let mut bindings = Vec::new();
    if secondary_flags != 0 || body.windows(7).any(|bytes| bytes == b"AB_ROOT") {
        let native = world_gpl::named_animation_bindings(body)?;
        let dynamics = secondary_chains(&glb, &native, name)?;
        if secondary_flags != 0 || !dynamics.chains.is_empty() {
            body_part.secondary_motion = dynamics;
            bindings = native;
        }
    }
    enemy_blend(&mut body_part, &alpha);
    body_part.mesh = format!("{base}/meshes/body.glb");
    files.insert(body_part.mesh.clone(), glb.clone());
    let mut meshes = vec![glb];
    let mut motion_bindings = vec![bindings];
    let mut parts = vec![PreviewPart {
        scene: body_part,
        animation: None,
        attached_to: None,
        additive: false,
        uv_offsets: Vec::new(),
    }];
    if let Some(outline) = outline {
        let owned_palette = dol::u32(outline, 0)? != 0;
        let outline = if owned_palette {
            outline.to_vec()
        } else {
            world_gpl::with_primary_palette(body, outline)?
        };
        let (outline_paths, outline_alpha) = if owned_palette {
            let tpl = palette(&outline)?;
            let mut paths = Vec::new();
            let mut alpha = Vec::new();
            for index in 0..dol::u32(tpl, 4)? as usize {
                let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
                let path = if let Some((path, _)) = files.iter().find(|(path, old)| {
                    path.starts_with(&format!("{base}/textures/")) && **old == texture
                }) {
                    path.clone()
                } else {
                    let path = format!("{base}/textures/outline-texture-{index}.ktx2");
                    files.insert(path.clone(), texture);
                    path
                };
                paths.push(path);
                alpha.push(transparent);
            }
            (paths, alpha)
        } else {
            (paths, alpha)
        };
        let (glb, mut outline_part) =
            world_gpl::static_model(&outline, outline_paths, outline_alpha.clone())?;
        let mut bindings = Vec::new();
        if secondary_flags != 0 || outline.windows(7).any(|bytes| bytes == b"AB_ROOT") {
            let native = world_gpl::named_animation_bindings(&outline)?;
            let dynamics = secondary_chains(&glb, &native, name)?;
            if secondary_flags != 0 || !dynamics.chains.is_empty() {
                outline_part.secondary_motion = dynamics;
                bindings = native;
            }
        }
        enemy_blend(&mut outline_part, &outline_alpha);
        outline_part.mesh = format!("{base}/meshes/outline.glb");
        outline_part.resource = 1;
        outline_part.outline_color = Some([0, 0, 0, 127]);
        for material in &mut outline_part.materials {
            material.cull = CullFace::Front;
            material.blend = true;
            material.draw_order += 65536;
        }
        files.insert(outline_part.mesh.clone(), glb.clone());
        meshes.push(glb);
        motion_bindings.push(bindings);
        parts.push(PreviewPart {
            scene: outline_part,
            animation: None,
            attached_to: None,
            additive: false,
            uv_offsets: Vec::new(),
        });
    }
    if secondary_flags != 0 {
        let secondary = member(&package, 0x180)?.context("enemy secondary body is missing")?;
        let source = if dol::u32(secondary, 0)? != 0 {
            secondary.to_vec()
        } else {
            world_gpl::with_primary_palette(body, secondary)?
        };
        let tpl = palette(&source)?;
        let mut textures = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..dol::u32(tpl, 4)? as usize {
            let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
            let path = format!("{base}/textures/part-2-texture-{index}.ktx2");
            ensure!(
                files.insert(path.clone(), texture).is_none(),
                "duplicate secondary texture"
            );
            textures.push(path);
            alpha.push(transparent);
        }
        let anchor = if secondary_flags & 0x8000 != 0 {
            Some(
                parts[0]
                    .scene
                    .bone_names
                    .iter()
                    .find(|name| name.starts_with("pa00"))
                    .context("enemy secondary body has no pa00 anchor")?
                    .clone(),
            )
        } else {
            None
        };
        for (index, model) in [
            (false, Some(source.as_slice())),
            (true, member(&package, 0x198)?),
        ] {
            let Some(model) = model else { continue };
            let model = if index {
                world_gpl::with_primary_palette(&source, model)?
            } else {
                source.clone()
            };
            let (glb, mut scene) =
                world_gpl::static_model(&model, textures.clone(), alpha.clone())?;
            enemy_blend(&mut scene, &alpha);
            let resource = u16::try_from(parts.len())?;
            scene.resource = resource;
            scene.mesh = format!("{base}/meshes/part-{resource}.glb");
            for material in &mut scene.materials {
                material.draw_order +=
                    u32::from(resource) * resonance_content::field::MODEL_DRAW_SPAN;
            }
            if index {
                scene.outline_color = Some([0, 0, 0, 127]);
                for material in &mut scene.materials {
                    material.cull = CullFace::Front;
                    material.blend = true;
                }
            }
            files.insert(scene.mesh.clone(), glb.clone());
            motion_bindings.push(world_gpl::named_animation_bindings(&model)?);
            meshes.push(glb);
            parts.push(PreviewPart {
                scene,
                animation: None,
                attached_to: anchor.clone(),
                additive: false,
                uv_offsets: Vec::new(),
            });
        }
    }
    animate_body(&mut parts, metadata)?;
    let count = (dol::u32(&package, 0x14)? as usize).max(31);
    ensure!(
        count <= (0x160 - 0x20) / 4,
        "enemy motion count exceeds table"
    );
    for slot in 0..count {
        let field = 0x20 + slot * 4;
        if dol::u32(&package, field)? == 0 {
            continue;
        }
        let motion = member(&package, field)?.context("missing enemy motion")?;
        ensure!(dol::u32(motion, 0)? == 0x007b_7960, "invalid enemy motion");
        for (index, part) in parts.iter_mut().enumerate() {
            if index >= 2 && slot < usize::from(metadata[0xbd]) {
                continue;
            }
            let unique_joints = if motion_bindings[index].is_empty() {
                duplicate_motion_joints(motion)?
            } else {
                None
            };
            // A motion can omit joints inserted in this body (for example a
            // carried weapon). Its compact track indices then cannot index
            // the full skeleton; resolve its native labels instead.
            let partial = usize::from(dol::u16(motion, 12)?)
                != Skeleton::from_glb(&meshes[index])?.bones.len();
            let encoded = if motion_bindings[index].is_empty() && unique_joints.is_none() && !partial {
                world_motion::encode(motion, &meshes[index])
            } else {
                let bindings = if motion_bindings[index].is_empty() {
                    let model = member(&package, 0x18 + index * 4)?
                        .context("enemy duplicate-joint motion has no model")?;
                    world_gpl::named_animation_bindings(model)?
                } else {
                    motion_bindings[index].clone()
                };
                let native = bindings
                    .iter()
                    .map(|(id, _, raw)| (*id, raw.clone()))
                    .collect::<Vec<_>>();
                let encoded = world_motion::encode_bound(motion, &meshes[index], &native)?;
                if let Some(unique) = unique_joints {
                    ensure!(
                        Motion::decode(&encoded)?.tracks.len() >= unique.saturating_sub(1),
                        "duplicate-joint enemy motion lost more than one named track: encoded {}, native unique {unique}",
                        Motion::decode(&encoded)?.tracks.len()
                    );
                }
                Ok(encoded)
            }
            .with_context(|| format!("enemy {id} part {index} motion {slot}"))?;
            if index >= 2 && Motion::decode(&encoded)?.tracks.is_empty() {
                continue;
            }
            let path = if let Some(path) = motions.get(&encoded) {
                path.clone()
            } else {
                let prefix = if index == 0 {
                    "".to_owned()
                } else {
                    format!("part-{index}-")
                };
                let path = format!("{base}/animations/{prefix}motion-{slot}.glb");
                motions.insert(encoded.clone(), path.clone());
                files.insert(path.clone(), encoded);
                path
            };
            let mut bound = clip(path.clone(), slot as u16);
            if !motion_bindings[index].is_empty() {
                bound.pose_chains = posed_chains(
                    motion,
                    &motion_bindings[index],
                    &part.scene.secondary_motion,
                )?;
            }
            part.scene.clips.push(bound);
        }
    }
    for (index, part) in parts.iter_mut().enumerate() {
        let initial = if index < 2 {
            0
        } else {
            u16::from(metadata[0xbd])
        };
        part.animation = part
            .scene
            .clips
            .iter()
            .any(|clip| clip.resource_slot == initial)
            .then_some(initial);
        part.scene.autoplay = part.animation.is_some();
    }
    if attachments != 0 {
        let bones = parts[0].scene.bone_names.clone();
        let pose_clips = carried_pose_clips(&package, attachments)?;
        for slot in 0..attachments {
            let resource = u16::try_from(parts.len())?;
            parts.push(carried_attachment(
                &package,
                metadata,
                body,
                name,
                &base,
                &mut files,
                motions,
                &bones,
                slot,
                resource,
                false,
                pose_clips.get(&slot),
            )?);
        }
        let first = member(&package, 0x160)?.context("first enemy carried model is missing")?;
        if dol::u32(first, 0)? == 6 && nested_section(first, 5)?.is_some() {
            let resource = u16::try_from(parts.len())?;
            parts.push(carried_attachment(
                &package, metadata, body, name, &base, &mut files, motions, &bones, 0, resource,
                true, None,
            )?);
        }
    }
    let (_, variant_files) = variant_models(&package, name, library)?;
    for (path, bytes) in variant_files {
        ensure!(
            files.insert(path, bytes).is_none(),
            "duplicate weapon variant media"
        );
    }
    let number = |at| -> Result<f32> { Ok(f32::from_bits(dol::u32(metadata, at)?)) };
    let hidden_geometry = parts[0]
        .scene
        .bone_names
        .iter()
        .filter(|name| {
            name.get(..2).is_some_and(|prefix| {
                prefix.eq_ignore_ascii_case("kk") || prefix.eq_ignore_ascii_case("pa")
            })
        })
        .cloned()
        .collect();
    let preview = ModelPreview {
        document: None,
        scale: number(0x11c)?,
        elevation: number(0x120)?,
        parts,
        hidden_geometry,
        behavior: preview_behavior(monster)?,
    };
    preview.validate_definition()?;
    files.insert(
        format!("{base}/model.json"),
        resonance_asset_writer::json::encode(&preview)?,
    );
    Ok(files)
}

/// Convert selected enemies together so identical performances have one owner.
pub fn cook_battle_enemy_media_group(root: &Path, library: &Path, ids: &[u8]) -> Result<()> {
    ensure!(!ids.is_empty(), "no battle enemies selected");
    let mut packages = EnemyPackages::open(&root.join("disc1"))?;
    let mut motions = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    let total = ids.len();
    for (index, id) in ids.into_iter().enumerate() {
        eprintln!(
            "[enemy media {}/{}] {} ({id})",
            index + 1,
            total,
            MonsterId::try_from(id)?
                .resource_name()
                .unwrap_or("unnamed")
        );
        ensure!(seen.insert(id), "battle enemy {id} is selected twice");
        let files = convert(&mut packages, id, &mut motions, library)?;
        ensure!(
            files.keys().all(|path| !library.join(path).exists()),
            "enemy {id} media already exists"
        );
        // Keep completed owners if a later native package needs a focused fix.
        for (path, bytes) in files {
            let output = library.join(path);
            crate::output::write(&output, &bytes)?;
        }
    }
    Ok(())
}

/// Convert one enemy in isolation, for a focused cook or inspection.
pub fn cook_battle_enemy_media(root: &Path, library: &Path, id: u8) -> Result<()> {
    cook_battle_enemy_media_group(root, library, &[id])
}
