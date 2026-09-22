//! Bind the title's static attachment skeletons and shared effect atlas.
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{Bone, Skeleton, Transform, TransformChannels};
use serde_json::Value;
use std::path::Path;

/// Resolve the geometry hierarchy once; sparse clips use the shared pose evaluator.
pub(crate) fn skeleton(gltf: &Value, bone_count: usize) -> Result<Skeleton> {
    #[derive(serde::Deserialize)]
    struct Node {
        name: String,
        #[serde(default)]
        children: Vec<u16>,
        translation: Option<[f32; 3]>,
        rotation: Option<[f32; 4]>,
        scale: Option<[f32; 3]>,
        matrix: Option<[f32; 16]>,
    }
    let nodes: Vec<Node> = gltf["nodes"]
        .as_array()
        .context("missing title skeleton")?
        .get(..bone_count)
        .context("title skeleton exceeds scene nodes")?
        .iter()
        .map(|node| serde_json::from_value(node.clone()))
        .collect::<Result<_, _>>()?;
    let mut parents = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        ensure!(
            node.matrix.is_none(),
            "expected authored TRS bind transforms"
        );
        for &child in node
            .children
            .iter()
            .filter(|&&child| usize::from(child) < bone_count)
        {
            let parent = parents
                .get_mut(usize::from(child))
                .context("missing glow child")?;
            ensure!(
                parent.replace(u16::try_from(index)?).is_none(),
                "glow node has multiple parents"
            );
        }
    }
    let skeleton = Skeleton {
        bones: nodes
            .into_iter()
            .zip(parents)
            .map(|(node, parent)| Bone {
                name: node.name,
                parent,
                bind_channels: TransformChannels(
                    u8::from(node.scale.is_some())
                        | (u8::from(node.rotation.is_some()) << 2)
                        | (u8::from(node.translation.is_some()) << 3),
                ),
                bind: Transform {
                    translation: node.translation.unwrap_or([0.; 3]),
                    rotation: node.rotation.unwrap_or([0., 0., 0., 1.]),
                    scale: node.scale.unwrap_or([1.; 3]),
                },
            })
            .collect(),
    };
    skeleton.validate()?;
    Ok(skeleton)
}

pub(crate) fn texture(
    extracted: &Path,
    output: &Path,
    resource: &crate::scene::title::Resource,
) -> Result<String> {
    let textures = resource.textures(extracted)?;
    let image = textures
        .catalogue
        .textures
        .get(2)
        .and_then(Option::as_ref)
        .context("effect atlas 2 missing")?
        .image(0)?;
    ensure!(
        image.width == 256 && image.height == 256,
        "unexpected glow atlas dimensions"
    );
    textures.write(output)?;
    Ok(image.path)
}

#[test]
#[ignore = "requires both original discs and frozen effect texture publications"]
fn original_glow_texture_matches_frozen_pixels_without_cooked_inputs() -> Result<()> {
    use std::fs;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let cooked = local.join("all-assets");
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    fs::create_dir(root.join("files"))?;
    let output = root.join("cooked");
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let recipe = crate::scene::title::Recipe::read(&extracted, &executable)?;
        let expected = crate::cooked::Source::open(&cooked, disc, &recipe.effects.path)?
            .cabinet_textures()?[2]
            .image(0)?;
        let actual = texture(&extracted, &output, &recipe.effects)?;
        crate::texture::compare_images(&output.join(&actual), &cooked.join(expected.path))?;
        fs::copy(
            extracted.join("files").join(&recipe.effects.path),
            root.join("files/Glow.cab"),
        )?;
        let mut renamed = crate::scene::title::Resource {
            path: "Glow.cab".into(),
            sha256: recipe.effects.sha256,
        };
        assert_eq!(texture(root, &output, &renamed)?, actual);
        renamed.path = "missing.cab".into();
        assert!(texture(root, &output, &renamed).is_err());
        renamed.path = "Glow.cab".into();
        renamed.sha256 = "0".repeat(64);
        assert!(texture(root, &output, &renamed).is_err());
    }
    Ok(())
}

#[test]
fn attachments_sample_sparse_keys_at_the_emitter_clock_and_keep_parent_placement() -> Result<()> {
    use resonance_content::animation::{FRAME_HZ, Motion};
    use serde_json::json;
    let skeleton = skeleton(
        &json!({"nodes": [
            {"name": "parent", "children": [1], "scale": [2., 1., 1.]},
            {"name": "attachment"}
        ]}),
        2,
    )?;
    for intervals in [1, 2, 4] {
        let times = (0..=intervals)
            .map(|key| key as f32 / intervals as f32)
            .collect::<Vec<_>>();
        let values = times
            .iter()
            .map(|time| [time * 2., 0., 0.])
            .collect::<Vec<_>>();
        let motion: Motion = serde_json::from_value(json!({
            "duration_frames": 1.,
            "tracks": [{
                "bone": 1, "bind_channels": 0, "period_frames": 1., "times": times,
                "translation": {"interpolation": "linear", "values": values}
            }]
        }))?;
        assert_eq!(
            (0..=3)
                .map(|tick| {
                    let frame = (tick as f32 * FRAME_HZ / resonance_content::ANIMATION_HZ)
                        .min(motion.duration_frames);
                    let point = skeleton.sample_point(&motion, frame, 1, [0.; 3])?;
                    Ok(std::array::from_fn(|i| {
                        (point[i] + [1.9, -1.9, 0.][i]).trunc()
                    }))
                })
                .collect::<Result<Vec<_>>>()?,
            [[1., -1., 0.], [3., -1., 0.], [5., -1., 0.], [5., -1., 0.]]
        );
    }
    Ok(())
}
