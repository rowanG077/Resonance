//! Falcon's Crest uses the shared GC finisher's authored clock.
use super::*;
use resonance_content::battle_martial::PoseStart;

pub(super) fn normalize(attack: &mut Attack) -> Result<()> {
    ensure!(
        attack.duration == 350,
        "Falcon's Crest source duration changed"
    );
    for (at, event) in &mut attack.events {
        // These parameters changed together with the longer native clock.
        // Restore the GC values with its launch, strike and recovery cues.
        match (*at, &mut *event) {
            (90, Event::Vertical(value)) => {
                ensure!(*value == 32., "Falcon's Crest source launch changed");
                *value = 28.;
            }
            (165, Event::AnimationRate(value)) => {
                ensure!(*value == 0.4, "Falcon's Crest source strike rate changed");
                *value = 0.5;
            }
            (183, Event::Gravity(value)) => {
                ensure!(
                    *value == -2.5,
                    "Falcon's Crest source strike gravity changed"
                );
                *value = -1.;
            }
            (260, Event::Gravity(value)) => {
                ensure!(
                    *value == -0.8,
                    "Falcon's Crest source recovery gravity changed"
                );
                // GC retains the preceding gravity during the closing leap.
                *value = -1.;
            }
            _ => {}
        }
        *at = match event {
            Event::AnimationRate(_) => match *at {
                92 => 92,
                165 => 155,
                _ => anyhow::bail!("Falcon's Crest source rate time changed"),
            },
            Event::Contact(_) => {
                ensure!(*at == 200, "Falcon's Crest source contact time changed");
                180
            }
            _ => match *at {
                0 | 10 | 75 | 90 => *at,
                122 => 98,
                // The added trail belongs to the downward strike, immediately
                // before its gravity command. Keep both at the GC strike cue.
                180 | 183 => 153,
                189 => 159,
                260 => 224,
                _ => anyhow::bail!("Falcon's Crest source command time changed"),
            },
        };
    }
    for step in attack
        .pose_sequence
        .as_mut()
        .context("missing Falcon's Crest poses")?
    {
        step.start = match (step.pose.clip, &step.start) {
            (80, PoseStart::At(0)) => PoseStart::At(0),
            (69, PoseStart::At(90)) => PoseStart::At(90),
            (70 | 0, PoseStart::AfterPrevious) => PoseStart::AfterPrevious,
            (28, PoseStart::At(253)) => PoseStart::At(223),
            (16, PoseStart::At(283)) => PoseStart::At(253),
            _ => anyhow::bail!("Falcon's Crest source pose sequence changed"),
        };
    }
    attack.duration = 300;
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; optionally writes RESONANCE_TEST_LIBRARY martial owner"]
    fn installed_falcons_crest_uses_shared_gc_timing_contract() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let catalogue = crate::battle_martial::read(&source)?;
        let document = resonance_importer::martial_attack_document(&catalogue)?;
        let restored = document.clone().prepare()?;
        let binding = restored.bindings[&228][&1];
        let attack = restored.variant(binding)?;
        assert_eq!(attack.duration, 300);
        assert!(attack.events.iter().any(|(at,event)| *at == 153 && matches!(event,Event::Trail { duration:60, .. })));
        assert!(
            attack
                .events
                .iter()
                .any(|(at, event)| *at == 155 && matches!(event, Event::AnimationRate(0.5)))
        );
        assert!(
            attack
                .events
                .iter()
                .any(|(at, event)| *at == 90 && matches!(event, Event::Vertical(28.)))
        );
        assert!(
            attack
                .events
                .iter()
                .any(|(at, event)| *at == 153 && matches!(event, Event::Gravity(-1.)))
        );
        for (at, event) in &attack.events {
            match event {
                Event::Contact(_) => assert_eq!(*at, 180),
                Event::AnimationRate(_) => assert!([92, 155].contains(at)),
                _ => assert!([0, 10, 75, 90, 98, 153, 159, 224].contains(at)),
            }
        }
        let poses = attack
            .pose_sequence
            .as_ref()
            .context("missing normalized poses")?;
        assert!(
            poses
                .iter()
                .any(|step| step.pose.clip == 28 && matches!(step.start, PoseStart::At(223)))
        );
        assert!(
            poses
                .iter()
                .any(|step| step.pose.clip == 16 && matches!(step.start, PoseStart::At(253)))
        );
        if let Some(library) = std::env::var_os("RESONANCE_TEST_LIBRARY") {
            output::json(
                &PathBuf::from(library).join(resonance_content::battle_rules::MARTIAL_ATTACKS),
                &document,
            )?;
        }
        Ok(())
    }
}
