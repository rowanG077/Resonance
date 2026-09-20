//! Source fields consumed by the shared stored-spell concluding motion.
use super::*;
use crate::battle::all::ActorSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fields {
    rate_bits: u32,
    start: u8,
    blend: u8,
    loop_start: u8,
    looping: bool,
}

impl Fields {
    fn read(actor: &ActorSettings) -> Result<Self> {
        let casting = &actor.casting;
        let rate = casting.animation_rate;
        ensure!(
            rate.is_finite() && rate > 0.,
            "unsupported stored resume playback rate"
        );
        Ok(Self {
            rate_bits: rate.to_bits(),
            start: casting.resume_start,
            blend: casting.resume_blend_ticks,
            loop_start: casting.resume_loop_start,
            looping: casting.release_looping,
        })
    }

    fn animation(self) -> Result<AnimationCommand> {
        ensure!(self.loop_start == 0, "unsupported stored resume loop start");
        Ok(AnimationCommand::Play {
            clip: 12,
            blend: self.blend,
            start: self.start,
            end: None,
            layer: 8,
            looping: self.looping,
            mirror: false,
            resource: -1,
            rate: f32::from_bits(self.rate_bits),
        })
    }
}

pub(super) fn character(tables: &Tables, character: u8) -> Result<AnimationCommand> {
    Fields::read(tables.actor(character)?)?.animation()
}

pub(super) fn shared(tables: &Tables, casters: &[CastRecipe]) -> Result<AnimationCommand> {
    let read = |caster: &CastRecipe| Fields::read(tables.actor(caster.character)?);
    let first = read(casters.first().context("stored resume has no caster")?)?;
    for caster in &casters[1..] {
        // Aliases may share a concluding animation even when their recovery clips differ.
        ensure!(
            read(caster)? == first,
            "stored spell aliases require distinct concluding animations"
        );
    }
    first.animation()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumed_fields_distinguish_resume_changes_from_adjacent_recovery_data() {
        let mut actor = ActorSettings::read(&[0; crate::battle::embedded::SETTINGS_BYTES]).unwrap();
        actor.casting.animation_rate = 0.5;
        actor.casting.resume_blend_ticks = 4;
        let expected = Fields::read(&actor).unwrap();
        actor.casting.stored_recovery_clip = 34;
        actor.casting.stored_release_looping = true;
        assert_eq!(Fields::read(&actor).unwrap(), expected);
        actor.casting.animation_rate = 1.;
        assert_ne!(Fields::read(&actor).unwrap(), expected);
        actor.casting.animation_rate = 0.5;
        for change in [
            (|actor: &mut ActorSettings| actor.casting.resume_start = 1) as fn(&mut ActorSettings),
            |actor| actor.casting.resume_blend_ticks = 8,
            |actor| actor.casting.resume_loop_start = 1,
            |actor| actor.casting.release_looping = true,
        ] {
            change(&mut actor);
            assert_ne!(Fields::read(&actor).unwrap(), expected);
            actor.casting.resume_start = 0;
            actor.casting.resume_blend_ticks = 4;
            actor.casting.resume_loop_start = 0;
            actor.casting.release_looping = false;
        }
        actor.casting.resume_loop_start = 1;
        assert!(Fields::read(&actor).unwrap().animation().is_err());
        actor.casting.animation_rate = 0.;
        assert!(Fields::read(&actor).is_err());
    }

    #[test]
    #[ignore = "requires original extracted disc; parses source records without encoding assets"]
    fn original_fire_aliases_keep_all_casters_and_their_release_motions() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let programs = technique_actions(&extracted, &rel, &usual, &[67, 68, 69, 214]).unwrap();
        let expected = [
            (67, 205, 24, 120, vec![3]),
            (68, 206, 55, 310, vec![3]),
            (69, 207, 24, 150, vec![3]),
            (214, 205, 24, 150, vec![6, 9]),
        ];
        for (program, (menu, native, tp, time, characters)) in programs.iter().zip(expected) {
            assert_eq!((program.technique, program.native_id), (menu, native));
            let TechniqueProgram::FireField {
                casters, resume, ..
            } = &program.program
            else {
                panic!("original fire field binding");
            };
            assert_eq!(
                casters.iter().map(|c| c.character).collect::<Vec<_>>(),
                characters
            );
            assert!(matches!(
                *resume,
                AnimationCommand::Play {
                    clip: 12,
                    blend: 4,
                    start: 0,
                    end: None,
                    layer: 8,
                    looping: false,
                    mirror: false,
                    resource: -1,
                    rate: 0.5,
                }
            ));
            for caster in casters {
                assert_eq!((caster.tp, caster.time_adjustment), (tp, time));
                assert_eq!((caster.pulse, caster.release_effect), (3, 7));
                assert!(caster.voices.begin != 0 && caster.voices.release != 0);
                assert!(caster.voices.begin_remaining >= 20);
                assert!(matches!(caster.release, Some(AnimationCommand::Play {
                    clip: 13, blend: 4, start: 0, end: None, layer: 8,
                    looping, mirror: false, resource: -1, rate: 0.5,
                }) if looping == (caster.character == 3)));
                let animations = if caster.character == 3 {
                    vec![(0, 30, 8, false), (52, 31, 2, false), (80, 32, 2, true)]
                } else {
                    vec![(0, 11, 8, true)]
                };
                assert_eq!(caster.animations.commands().count(), animations.len());
                for (index, (tick, clip, blend, looping)) in animations.into_iter().enumerate() {
                    let command = if index == 0 {
                        caster.animations.initial.unwrap()
                    } else {
                        let AnimationInstruction::Step(step) =
                            &caster.animations.instructions[&(index as i16)]
                        else {
                            panic!("expected motion")
                        };
                        assert!(
                            matches!(step.trigger, AnimationTrigger::Tick(value) if value == tick)
                        );
                        step.command
                    };
                    assert!(matches!(command, AnimationCommand::Play {
                        clip: value, blend: frames, start: 0, end: None, layer: 8,
                        looping: repeats, mirror: false, resource: -1, rate: 0.5,
                    } if (value, frames, repeats) == (clip, blend, looping)));
                }
            }
        }
        let tables = Tables::original(&extracted, &rel, &usual, &[]).unwrap();
        let genis = tables.actor(3).unwrap();
        let zelos = tables.actor(6).unwrap();
        let kratos = tables.actor(9).unwrap();
        assert_eq!(
            (
                genis.casting.stored_recovery_clip,
                zelos.casting.stored_recovery_clip,
                kratos.casting.stored_recovery_clip
            ),
            (0, 0, 34)
        );
        assert_eq!(Fields::read(genis).unwrap(), Fields::read(zelos).unwrap());
        assert_eq!(Fields::read(genis).unwrap(), Fields::read(kratos).unwrap());
    }
}
