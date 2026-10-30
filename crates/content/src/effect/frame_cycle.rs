//! Atlas frames with durations, expanded once when preparing their playback.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    frame: u8,
    ticks: u16,
}

pub(super) fn serialize<S: Serializer>(frames: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    if !(1..=1024).contains(&frames.len()) {
        return Err(serde::ser::Error::custom(
            "frame cycle must contain 1..=1024 ticks",
        ));
    }
    let mut steps: Vec<Step> = Vec::new();
    for &frame in frames {
        if let Some(previous) = steps.last_mut().filter(|step| step.frame == frame) {
            previous.ticks += 1;
        } else {
            steps.push(Step { frame, ticks: 1 });
        }
    }
    steps.serialize(serializer)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let steps = Vec::<Step>::deserialize(deserializer)?;
    let mut frames = Vec::new();
    for step in steps {
        if step.ticks == 0 || frames.len() + usize::from(step.ticks) > 1024 {
            return Err(serde::de::Error::custom(
                "frame cycle must contain 1..=1024 ticks",
            ));
        }
        frames.extend(std::iter::repeat_n(step.frame, usize::from(step.ticks)));
    }
    if frames.is_empty() {
        return Err(serde::de::Error::custom("frame cycle is empty"));
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::super::BlinkCycle;

    #[test]
    fn timed_frames_preserve_loop_phase_and_reject_invalid_durations() -> anyhow::Result<()> {
        let source = r#"[{"frame":0,"ticks":4},{"frame":1,"ticks":4},{"frame":2,"ticks":4},{"frame":0,"ticks":121}]"#;
        let cycle: BlinkCycle = serde_json::from_str(source)?;
        assert_eq!(cycle.frames.len(), 133);
        for tick in 0..399 {
            assert_eq!(
                cycle.frames[tick % 133],
                match tick % 133 {
                    4..=7 => 1,
                    8..=11 => 2,
                    _ => 0,
                }
            );
        }
        assert_eq!(serde_json::to_string(&cycle)?, source);
        for invalid in [
            "[]",
            r#"[{"frame":0,"ticks":0}]"#,
            r#"[{"frame":0,"ticks":1025}]"#,
            r#"[{"frame":0,"ticks":1024},{"frame":1,"ticks":1}]"#,
            r#"{"frames":[0,1]}"#,
        ] {
            assert!(
                serde_json::from_str::<BlinkCycle>(invalid).is_err(),
                "{invalid}"
            );
        }
        Ok(())
    }
}
