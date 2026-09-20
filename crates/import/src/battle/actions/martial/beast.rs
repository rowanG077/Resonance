//! Beast and Hunting Beast customize shared tracks once at entry.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    pub native: u16,
    duration: u16,
    voice: u16,
    opening_voices: Option<[u16; 2]>,
}

const SHOULDER: &[u8] = b"Bone_kata_R\0";

fn body(rel: &Rel, entry: usize, expected: &[u32]) -> Result<()> {
    let code = rel.at((1, entry))?;
    for (index, instruction) in expected.iter().copied().enumerate() {
        ensure!(
            word(code, index * 4)? == instruction,
            "changed Beast initializer at {:#x}",
            entry + index * 4
        );
    }
    Ok(())
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<MartialCallback> {
    ensure!(
        variant == 0
            && source.phases[0].duration == p.duration
            && (1..4).all(|index| source.phases[index].duration == 0),
        "unexpected Beast variants"
    );
    let hits = source.hits(0)?;
    ensure!(
        matches!(
            hits.first(),
            Some(HitWindow {
                start: 12,
                emission: HitEmission::Contact {
                    duration: 8,
                    attachment: HitAttachment::BodyBone(34)
                },
                ..
            })
        ),
        "changed Beast first-hit binding"
    );
    let (commands, _) = source.commands(0)?;
    ensure!(
        matches!(commands.first(), Some(TimedCommand {
        tick: 0, command: ActionCommand::Voice { id, priority: 2 }
    }) if *id == p.voice),
        "changed Beast first-voice binding"
    );
    Ok(MartialCallback::Beast {
        opening_voices: p.opening_voices,
    })
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (entry, name, duration, voice) = match native {
        20 => (0x64870, 0x3d80, 75, 0x804e),
        22 => (0x648fc, 0x3de0, 125, 0x8050),
        _ => bail!("unsupported Beast family sibling"),
    };
    let dispatch = rel.pointer(DATA, 0xd60 + usize::from(native) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, entry)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected Beast dispatch"
    );
    ensure!(
        rel.at((4, name))?.starts_with(SHOULDER),
        "changed Beast shoulder name"
    );
    if native == 20 {
        body(
            rel,
            entry,
            &[
                0x9421ffe0, 0x7c0802a6, 0x90010024, 0x93e1001c, 0x93c10018, 0x93a10014, 0x7c7d1b78,
                0x4bfd39b1, 0x3c800000, 0x807d02c0, 0x38840000, 0x4bff6e9d, 0x809d0014, 0x7c7f1b78,
                0x83c40018, 0x4bfe9595, 0x546007bf, 0x40820010, 0x3800006f, 0xb01e0004, 0x48000010,
                0x3c600001, 0x3803804e, 0xb01e0004, 0x807d0014, 0x80630010, 0x9be30005, 0x80010024,
                0x83e1001c, 0x83c10018, 0x83a10014, 0x7c0803a6, 0x38210020, 0x4e800020,
            ],
        )?;
    } else {
        body(
            rel,
            entry,
            &[
                0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x7c7f1b78, 0x4bfd392d, 0x3c800000,
                0x807f02c0, 0x38840000, 0x4bff6e19, 0x80bf0014, 0x3c800000, 0x38040000, 0x80850010,
                0x98640005, 0x901f0018, 0x80010014, 0x83e1000c, 0x7c0803a6, 0x38210010, 0x4e800020,
            ],
        )?;
        ensure!(
            word(rel.at((1, 0x648f8))?, 0)? == 0x4e800020
                && rel.local_targets().contains(&(1, 0x648f8)),
            "Hunting Beast callback is no longer empty"
        );
    }
    Ok(Parameters {
        native,
        duration,
        voice,
        opening_voices: (native == 20).then_some([111, 0x804e]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the original extracted disc"]
    fn original_beast_entries_retain_authored_variants_and_guard_every_native_side_effect() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let actions = technique_actions(&extracted, &rel, &usual, &[20, 22]).unwrap();
        for (action, duration, effect, combo) in actions
            .iter()
            .zip([75, 125])
            .zip([33, 40])
            .zip([65, 80])
            .map(|(((action, duration), effect), combo)| (action, duration, effect, combo))
        {
            let TechniqueProgram::Martial { variants } = &action.program else {
                panic!()
            };
            assert_eq!(variants.len(), 1);
            let phase = &variants[0];
            assert_eq!(
                (
                    phase.variant,
                    phase.action.duration,
                    phase.effect,
                    phase.combo_at
                ),
                (0, duration, Some(effect), combo)
            );
            assert_eq!(phase.recovery_ticks, 10);
            let source = member(member(&usual, 8).unwrap(), usize::from(action.native_id)).unwrap();
            assert!(
                callback(
                    &read_parameters(&rel, action.native_id).unwrap(),
                    &bundle::Bundle::decode(source).unwrap(),
                    1
                )
                .is_err()
            );
        }
        assert!(read_parameters(&rel, 21).is_err());
        let catalogue = BattleActions {
            party: vec![],
            enemies: vec![],
            projectiles: vec![],
            techniques: actions,
            chains: None,
        };
        catalogue.validate().unwrap();
        let (_, voices) = catalogue.audio_ids();
        assert_eq!(
            voices.into_iter().collect::<Vec<_>>(),
            [111, 0x804e, 0x8050]
        );
        for (native, address, replacement) in [
            (20, 0x648ac, 0x60000000_u32), // removing the RNG draw changes later RNG consumers
            (20, 0x648d8, 0x9be30006),     // a different hit field is not this binding
            (22, 0x648f8, 0x60000000),     // Hunting Beast's installed callback must stay empty
        ] {
            let at = rel.sections[1].0 + address;
            let old: [u8; 4] = rel.bytes[at..at + 4].try_into().unwrap();
            rel.bytes[at..at + 4].copy_from_slice(&replacement.to_be_bytes());
            assert!(read_parameters(&rel, native).is_err());
            rel.bytes[at..at + 4].copy_from_slice(&old);
        }
    }
}
