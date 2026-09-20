use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    direction: [f32; 3],
    distance: f32,
    color: [u8; 4],
    end_bones: Vec<String>,
}

pub(super) fn read_parameters(rel: &Rel, kind: PowWeapon) -> Result<Parameters> {
    let (initializer, direction, distance, color, bones) = match kind {
        PowWeapon::Blade => (0x7de30, 0x6020, 0x6040, 0x602c, vec![0x6030, 0x6038]),
        PowWeapon::Devastation => (0x8b1cc, 0x8ae0, 0x8af8, 0x8aec, vec![0x8af0]),
        PowWeapon::Spear => (0x8b7a8, 0x8bb8, 0x8bd0, 0x8bc4, vec![0x8bc8]),
    };
    let dispatch = rel.pointer(5, 0xf94 + usize::from(kind.native() - 300) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected Pow dispatch"
    );
    if kind == PowWeapon::Devastation {
        ensure!(
            float(rel.at((4, 0x8afc))?, 0)? == 1.,
            "unsupported Pow Devastation startup scale"
        );
    }
    let text = |offset| -> Result<String> {
        let bytes = rel.at((4, offset))?;
        let end = bytes
            .iter()
            .position(|&b| b == 0)
            .context("unterminated Pow bone")?;
        Ok(std::str::from_utf8(&bytes[..end])?.to_owned())
    };
    Ok(Parameters {
        direction: [
            float(rel.at((4, direction))?, 0)?,
            float(rel.at((4, direction))?, 4)?,
            float(rel.at((4, direction))?, 8)?,
        ],
        distance: float(rel.at((4, distance))?, 0)?,
        color: rel.at((4, color))?[..4].try_into()?,
        end_bones: bones.into_iter().map(text).collect::<Result<_>>()?,
    })
}

pub(super) fn cook(inputs: &Inputs, kind: PowWeapon) -> Result<PowProgram> {
    let parameters = inputs
        .parameters
        .pow
        .get(&kind)
        .context("missing Pow parameters")?;
    let package = inputs.package(kind.native())?;
    ensure!(
        package.resources.models == [0; 10] && package.resources.callback_resources[1..] == [0; 3],
        "unexpected Pow native resource"
    );
    let source = &package.actions;
    let phases = (0..kind.characters().len())
        .map(|index| phase(source, index))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        source.phases[phases.len()..]
            .iter()
            .all(|phase| phase.duration == 0),
        "unexpected populated Pow role"
    );
    let data = PowProgram {
        phases,
        direction: parameters.direction,
        distance: parameters.distance,
        color: parameters.color,
        end_bones: parameters.end_bones.clone(),
    };
    data.validate(kind)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Original Magic101 action bundle, separately bounded by resource fields256/260.
    const BUNDLE: &[u8] = &[
        0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x9c, 0x00, 0x00, 0x01, 0x1c, 0x00, 0x00, 0x01,
        0x70, 0x00, 0x3c, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x3c, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x16, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x23, 0x1e, 0x05, 0x01,
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x2c, 0x00, 0x6b, 0x00, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x41,
        0xf0, 0x00, 0x00, 0x41, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x07, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x0a, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x41, 0xf0, 0x00, 0x00, 0x41, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x2e, 0x04, 0x00, 0x00, 0x08, 0xff, 0x3e, 0xcc, 0xcc, 0xcd, 0x00, 0x0e, 0x2f, 0x04,
        0x00, 0x00, 0x08, 0xff, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x1e, 0x2a, 0x02, 0x00, 0x00, 0x08,
        0xff, 0x3f, 0x00, 0x00, 0x00, 0xff, 0xfd, 0x00, 0x08, 0x00, 0x00, 0x48, 0xff, 0x3f, 0x00,
        0x00, 0x00, 0xff, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x2e, 0x04, 0x00, 0x00, 0x08, 0xff, 0x3f, 0x00, 0x00, 0x00, 0xff, 0xfe, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1b, 0x87, 0x95, 0x00,
        0x02, 0x00, 0x00, 0x00, 0x1c, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32,
        0x00, 0x02, 0x00, 0x01, 0x00, 0xb9, 0x00, 0x02, 0x00, 0x03, 0xff, 0xf4, 0x00, 0x14, 0x00,
        0x1c, 0x00, 0x3c, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    #[test]
    fn original_roles_keep_two_timed_contacts_and_the_hidden_empty_phase() {
        let bundle = Bundle::decode(BUNDLE).unwrap();
        let first = phase(&bundle, 0).unwrap();
        let second = phase(&bundle, 1).unwrap();
        let mut no_effect = BUNDLE.to_vec();
        no_effect[24..28].fill(0);
        assert!(
            phase(&Bundle::decode(&no_effect).unwrap(), 0)
                .unwrap()
                .effect
                .is_none()
        );
        assert_eq!(
            (
                first.action.duration,
                second.action.duration,
                first.recovery_ticks,
                second.recovery_ticks
            ),
            (60, 60, 20, 20)
        );
        assert!(first.effect.is_none() && second.effect.is_none());
        assert!(first.callback.is_none() && second.callback.is_none());
        assert_eq!(
            first
                .action
                .hits
                .iter()
                .map(|hit| (hit.start, hit.shape.reaction, hit.rule.power))
                .collect::<Vec<_>>(),
            [(0, 7, 300), (18, 1, 300)]
        );
        assert!(
            matches!(&first.action.hits[0].emission, HitEmission::Contact { duration: 8, attachment: HitAttachment::Groups(groups) } if groups == &[0])
        );
        assert!(
            matches!(&first.action.hits[1].emission, HitEmission::Contact { duration: 10, attachment: HitAttachment::Groups(groups) } if groups == &[1])
        );
        assert!(
            matches!(first.action.animations.initial.unwrap(), AnimationCommand::Play { clip:46,blend:4,rate,.. } if rate == 0.4)
        );
        assert!(
            matches!(second.action.animations.initial.unwrap(), AnimationCommand::Play { clip:46,blend:4,rate,.. } if rate == 0.5)
        );
        assert!(second.action.commands.is_empty() && second.action.hits.is_empty());
        assert_eq!(first.action.commands.len(), 6);
        assert!(matches!(
            first.action.commands[0].command,
            ActionCommand::Voice {
                id: 0x8795,
                priority: 2
            }
        ));
        assert!(
            matches!(first.action.commands[4].command, ActionCommand::Gravity(value) if (value+1.2).abs()<0.000001)
        );
    }
}

#[cfg(test)]
mod followups {
    use super::*;
    use resonance_content::battle::actions::AnimationTrigger;
    const DEVASTATION: &str = "00000080000000b800000138000001800055001400000000ffffffff00000000000000000000000000000000003c00140000
0000ffffffff0000000000000003000000040000001e0000000000000000ffffffff00000000000000000000000000000000
0000000000000000ffffffff00000000000000000000000000000000002000231e050101000000000001012c006b00000000
030006000000002800231e050101000000000001012c006b0000000003000600000000160e01000000004220000042200000
0000000101000000000000000000000000320e0100000000422000004220000000000101110000000000000000000000ffff
000000000000000000000000000000000000000000000000000000000000ffff000000000000000000000000000000000000
00000000000000000000000000000e04000008ff3f00000000064604000008ff3f00000000282804000008ff3f000000fffe
0000000000000000000000002e04000008ff3f000000fffe0000000000000000000000080001010400080003ffec00080000
000a000c001b02bb0002000e001c003f0000001e00000000002c001b02bc0002002e001c003f0000ffffffffffffffff";
    const SPEAR: &str = "00000080000000b800000218000002900055001400000000ffffffff00000000000000000000000000000000005500140000
0000ffffffff00000000000000050000000400000024003c001400000000ffffffff000000000000000a0000000800000048
0000000000000000ffffffff00000000000000000000000000000000002000231e0501010000000000010064006b00000000
020006000000002800231e050101000000000001012c006b0000000002000600000000060301ff00000042d2000042d20000
000000010a0000000000000000000000000b0301ff00000042d2000042d20000000000010b00000000000000000000000010
0301ff00000042d2000042d200000000000107000000000000000000000000240a0100000000423400004234000000000101
020000000000000000000000ffff00000000000000000000000000000000000000000000000000000000000000060301ff00
000042d2000042d20000000000010a0000000000000000000000000b0301ff00000042d2000042d20000000000010b000000
000000000000000000100301ff00000042d2000042d200000000000107000000000000000000000000240a01000000004234
00004234000000000101020000000000000000000000ffff0000000000000000000000000000000000000000000000000000
00000000ffff00000000000000000000000000000000000000000000000000000000000000003304000008ff3f0000000016
3503000008ff3e99999a004e1004000008ff3f000000fffe0000000000000000000000003304000008ff3f00000000163503
000008ff3e99999a004e1004000008ff3f000000fffe0000000000000000000000002e04000008ff3f000000fffe00000000
0000000000000000001b87b800020000000d0000005a0008001c003c000000080000000a0008000100af00080003fff80026
001c003c00000026000100af00260003fff600260000ffecffffffff0000001b83f800020000000d0000005a0008001c003c
000000080000000a0008000100af00080003fff80026001c003c00000026000100af00260003fff600260000ffecffffffff
ffffffff0000000000000000c0ff1200b336400003000000100e3600480e3600";
    fn bytes(text: &str) -> Vec<u8> {
        let hex = text.split_whitespace().collect::<String>();
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    fn motions(
        phase: &resonance_content::battle::actions::TechniquePhase,
    ) -> Vec<(i16, u8, u8, f32)> {
        use resonance_content::battle::actions::AnimationInstruction;
        let program = &phase.action.animations;
        program
            .initial
            .into_iter()
            .map(|command| (0, command))
            .chain(
                program
                    .instructions
                    .values()
                    .filter_map(|instruction| match instruction {
                        AnimationInstruction::Step(step) => {
                            let AnimationTrigger::Tick(tick) = step.trigger else {
                                panic!("unexpected motion trigger")
                            };
                            Some((tick, step.command))
                        }
                        AnimationInstruction::End => None,
                        AnimationInstruction::Stalled => panic!("unexpected stalled program"),
                    }),
            )
            .map(|(tick, command)| {
                let AnimationCommand::Play {
                    clip, blend, rate, ..
                } = command
                else {
                    panic!("unexpected motion command")
                };
                (tick, clip, blend, rate)
            })
            .collect()
    }
    fn contacts(
        phase: &resonance_content::battle::actions::TechniquePhase,
    ) -> Vec<(u16, u8, u8, u16)> {
        phase
            .action
            .hits
            .iter()
            .map(|hit| {
                let HitEmission::Contact { duration, .. } = hit.emission else {
                    panic!("unexpected emission")
                };
                (hit.start, duration, hit.shape.reaction, hit.rule.power)
            })
            .collect()
    }
    #[test]
    fn original_pow_followups_keep_character_tracks_and_distinct_hit_windows() {
        let devastation = Bundle::decode(&bytes(DEVASTATION)).unwrap();
        let first = phase(&devastation, 0).unwrap();
        assert_eq!((first.action.duration, first.recovery_ticks), (85, 20));
        assert_eq!(
            motions(&first),
            [(0, 14, 4, 0.5), (6, 70, 4, 0.5), (40, 40, 4, 0.5)]
        );
        assert_eq!(contacts(&first), [(22, 14, 1, 300), (50, 14, 17, 300)]);
        assert!(first.action.hits.iter().all(|hit| matches!(&hit.emission,HitEmission::Contact {attachment:HitAttachment::Groups(groups),..} if groups==&[0]) && hit.rule.impact_effect==3 && hit.rule.impact_bank==6));
        assert!(matches!(
            first.action.commands[0].command,
            ActionCommand::VerticalSpeed(26.)
        ));
        assert!(matches!(
            first.action.commands[1].command,
            ActionCommand::Gravity(-2.)
        ));
        assert_eq!(
            first
                .action
                .commands
                .iter()
                .filter_map(
                    |step| if let ActionCommand::Voice { id, priority } = step.command {
                        Some((step.tick, id, priority))
                    } else {
                        None
                    }
                )
                .collect::<Vec<_>>(),
            [(12, 699, 2), (44, 700, 2)]
        );
        let spear = Bundle::decode(&bytes(SPEAR)).unwrap();
        for (index, voice) in [(0, 0x87b8), (1, 0x83f8)] {
            let first = phase(&spear, index).unwrap();
            assert_eq!((first.action.duration, first.recovery_ticks), (85, 20));
            assert_eq!(
                motions(&first),
                [(0, 51, 4, 0.5), (22, 53, 3, 0.3), (78, 16, 4, 0.5)]
            );
            assert_eq!(
                contacts(&first),
                [
                    (6, 3, 10, 100),
                    (11, 3, 11, 100),
                    (16, 3, 7, 100),
                    (36, 10, 2, 300)
                ]
            );
            assert!(first.action.hits[..3].iter().all(|hit| matches!(
                hit.emission,
                HitEmission::Contact {
                    attachment: HitAttachment::Center,
                    ..
                }
            ) && hit.shape.radius == 105.));
            assert!(
                first
                    .action
                    .hits
                    .iter()
                    .all(|hit| hit.rule.impact_effect == 2 && hit.rule.impact_bank == 6)
            );
            assert!(
                matches!(first.action.commands[0].command,ActionCommand::Voice {id,priority:2} if id==voice)
            );
            assert!(matches!(
                first.action.commands[1].command,
                ActionCommand::AttachmentTrail { slot: 0, ticks: 90 }
            ));
            assert!(matches!(
                first.action.commands[9].command,
                ActionCommand::ForwardSpeed(-2.)
            ));
        }
        for (bytes, index) in [(&devastation, 1), (&spear, 2)] {
            let hidden = phase(bytes, index).unwrap();
            assert_eq!((hidden.action.duration, hidden.recovery_ticks), (60, 20));
            assert_eq!(motions(&hidden), [(0, 46, 4, 0.5)]);
            assert!(
                hidden.effect.is_none()
                    && hidden.action.commands.is_empty()
                    && hidden.action.hits.is_empty()
            );
        }
    }
}
