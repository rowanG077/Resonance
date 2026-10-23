//! Translate original entry selection; extract native saved-coordinate constants.
use crate::{read, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{EntryOverride, EntryPlacement, Landmarks, Mount};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Recipes {
    provenance: String,
    entries: BTreeMap<u16, Source>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    direction: Option<u8>,
    placement: EntryPlacement,
    mount: Option<Mount>,
    height_offset: Option<usize>,
    heading_offset: Option<usize>,
    camera_yaw_offset: Option<usize>,
}

pub(super) fn bind(landmarks: &mut Landmarks, module: &Rel) -> Result<()> {
    let recipes: Recipes = serde_json::from_str(include_str!("overworld_entries.json"))?;
    ensure!(
        !recipes.provenance.is_empty(),
        "missing world entry provenance"
    );
    let value = |offset| read::f32(module.at((5, offset))?, 0);
    let heading_scale = value(0x958)?;
    let camera_scale = value(0xa0c)?;
    ensure!(
        heading_scale > 0.
            && heading_scale.is_finite()
            && camera_scale > 0.
            && camera_scale.is_finite(),
        "invalid saved entry angle scales"
    );
    for (id, source) in recipes.entries {
        let landmark = landmarks
            .worlds
            .iter_mut()
            .flatten()
            .find(|entry| entry.id == id)
            .with_context(|| format!("entry override references missing landmark {id}"))?;
        ensure!(
            landmark.entry_overrides.is_empty(),
            "duplicate entry recipe {id}"
        );
        let mut placement = source.placement;
        if let Some(offset) = source.height_offset {
            let EntryPlacement::LandmarkCenter { height } = &mut placement else {
                anyhow::bail!("height constant needs landmark center placement");
            };
            *height = value(offset)?.trunc() / 100.;
        }
        let rule = EntryOverride {
            direction: source.direction,
            placement,
            mount: source.mount,
            heading: source
                .heading_offset
                .map(|offset| -> Result<f32> { Ok(value(offset)?.trunc() / heading_scale) })
                .transpose()?,
            camera_yaw: source
                .camera_yaw_offset
                .map(|offset| -> Result<f32> { Ok(value(offset)?.trunc() / camera_scale) })
                .transpose()?,
        };
        rule.validate()?;
        landmark.entry_overrides.push(rule);
    }
    landmarks.validate()
}

#[test]
#[ignore = "requires both original extracted discs; CPU only"]
fn original_landmark_entry_recipes_extract_saved_units_and_keep_disabled_embark() -> Result<()> {
    use std::{fs, path::Path};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut first = None;
    for disc in [1, 2] {
        let executable = fs::read(root.join(format!("disc{disc}/sys/main.dol")))?;
        let mut landmarks =
            super::overworld_landmarks::prepare(&super::world_map::read(&executable)?)?;
        let module = Rel::read(&root.join(format!("disc{disc}/files/US_r_Top2field.rel")))?;
        bind(&mut landmarks, &module)?;
        let rules: BTreeMap<_, _> = landmarks
            .worlds
            .iter()
            .flatten()
            .filter(|entry| !entry.entry_overrides.is_empty())
            .map(|entry| (entry.id, entry.entry_overrides.clone()))
            .collect();
        assert_eq!(
            rules.keys().copied().collect::<Vec<_>>(),
            [2, 264, 266, 280, 304]
        );
        assert_eq!(rules[&2][0].direction, Some(6));
        assert_eq!(
            rules[&2][0].placement,
            EntryPlacement::Fixed {
                position: [9770., 24160., 0.]
            }
        );
        assert_eq!(rules[&2][0].heading, Some(12867. / 4096.));
        assert_eq!(rules[&2][0].camera_yaw, Some(100. / 32.));
        assert_eq!(
            rules[&264][0].placement,
            EntryPlacement::Perimeter {
                allow_disabled: true
            }
        );
        assert_eq!(rules[&264][0].mount, Some(Mount::Ship));
        assert_eq!(
            rules[&266][0].placement,
            EntryPlacement::LandmarkCenter { height: 500. }
        );
        assert_eq!(
            rules[&304][0].placement,
            EntryPlacement::LandmarkCenter { height: 0. }
        );
        for id in [266, 304] {
            assert_eq!(rules[&id][0].mount, Some(Mount::Rheairds));
            assert_eq!(rules[&id][0].heading, Some(0.));
            assert_eq!(rules[&id][0].camera_yaw, Some(0.));
        }
        assert_eq!(
            rules[&280][0].placement,
            EntryPlacement::Fixed {
                position: [36930., 23915., 0.]
            }
        );
        assert_eq!(rules[&280][0].heading, Some(3216. / 4096.));
        assert_eq!(rules[&280][0].camera_yaw, Some(175. / 32.));
        if let Some(previous) = first.replace(rules.clone()) {
            assert_eq!(previous, rules);
        }
    }
    Ok(())
}
