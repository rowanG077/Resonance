use super::*;
use anyhow::Result;
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct Clips(#[serde(with = "super")] Vec<SceneClip>);
fn clip(bank: Option<u32>, slot: u16, motion: &str) -> SceneClip {
    SceneClip {
        joints: Default::default(),
        motion: motion.into(),
        resource_slot: slot,
        animation_resource: bank,
        duration_seconds: 0.,
        pose_chains: vec![],
    }
}
fn requests(clips: &[SceneClip]) -> BTreeMap<(Option<u32>, u16), Value> {
    clips
        .iter()
        .map(|c| {
            (
                (c.animation_resource, c.resource_slot),
                serde_json::to_value(c).unwrap(),
            )
        })
        .collect()
}
fn first_slots(clips: &[SceneClip]) -> BTreeMap<u16, Value> {
    let mut first = BTreeMap::new();
    for c in clips {
        first
            .entry(c.resource_slot)
            .or_insert_with(|| json!([c.motion, c.joints, c.duration_seconds, c.pose_chains]));
    }
    first
}

#[test]
fn compact_bindings_preserve_requests_autoplay_and_first_slot_priority_in_every_order() -> Result<()>
{
    let mut clips = vec![
        clip(None, 0, "a.glb"),
        clip(None, 12, "b.glb"),
        clip(Some(7), 0, "b.glb"),
        clip(Some(7), 12, "a.glb"),
        clip(Some(8), 0, "b.glb"),
        clip(Some(8), 12, "a.glb"),
    ];
    fn permutations(clips: &mut [SceneClip], at: usize, count: &mut usize) -> Result<()> {
        if at == clips.len() {
            let encoded = serde_json::to_value(Clips(clips.to_vec()))?;
            let decoded: Clips = serde_json::from_value(encoded.clone())?;
            assert_eq!(requests(clips), requests(&decoded.0));
            assert_eq!(first_slots(clips), first_slots(&decoded.0));
            assert_eq!(clips[0], decoded.0[0]);
            assert_eq!(encoded, serde_json::to_value(decoded)?);
            *count += 1;
            return Ok(());
        }
        for i in at..clips.len() {
            clips.swap(at, i);
            permutations(clips, at + 1, count)?;
            clips.swap(at, i);
        }
        Ok(())
    }
    let mut count = 0;
    permutations(&mut clips, 0, &mut count)?;
    assert_eq!(count, 720);
    Ok(())
}

#[test]
fn shared_clip_edits_reach_exact_slots_and_banks_and_keep_distinct_pose_chains() -> Result<()> {
    let mut clips = vec![
        clip(None, 12, "idle.glb"),
        clip(None, 36, "idle.glb"),
        clip(Some(1), 12, "greeting.glb"),
        clip(Some(2), 12, "greeting.glb"),
        clip(Some(2), 16, "different.glb"),
    ];
    let mut other = clip(Some(3), 12, "greeting.glb");
    other.pose_chains = vec!["hair".into()];
    other.joints.insert("hips".into(), Some("waist".into()));
    other.joints.insert("cape".into(), None);
    clips.push(other);
    let mut value = serde_json::to_value(Clips(clips))?;
    assert_eq!(value.as_array().unwrap().len(), 4);
    assert_eq!(value[0], json!({"motion":"idle.glb","slots":[12,36]}));
    assert_eq!(
        value[1],
        json!({"motion":"greeting.glb","slots":[12],"banks":[1,2]})
    );
    value[1]["motion"] = json!("modded.glb");
    let decoded: Clips = serde_json::from_value(value)?;
    let targets = requests(&decoded.0);
    assert_eq!(targets[&(Some(1), 12)]["motion"], "modded.glb");
    assert_eq!(targets[&(Some(2), 12)]["motion"], "modded.glb");
    assert!(!targets.contains_key(&(Some(1), 16)));
    assert_eq!(targets[&(Some(2), 16)]["motion"], "different.glb");
    assert_eq!(targets[&(Some(3), 12)]["pose_chains"], json!(["hair"]));
    assert_eq!(
        targets[&(Some(3), 12)]["joints"],
        json!({"hips":"waist", "cape":null})
    );
    Ok(())
}

#[test]
fn bindings_reject_missing_duplicate_and_old_request_shapes() {
    for rows in [
        json!([{"motion":"a.glb","slots":[]}]),
        json!([{"motion":"a.glb","slots":[12,12]}]),
        json!([{"motion":"a.glb","slots":[12],"banks":[1,1]}]),
        json!([{"motion":"a.glb","slots":[12]},{"motion":"b.glb","slots":[12]}]),
        json!([{"motion":"a.glb","resource_slot":12}]),
        json!([{"motion":"a.glb","slots":[12],"animation_resource":1}]),
        json!([{"motion":"../outside.glb","slots":[12]}]),
    ] {
        assert!(serde_json::from_value::<Clips>(rows).is_err());
    }
}
