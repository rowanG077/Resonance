//! A registered defeat monitor does not imply a registered entrance scene.
use super::*;
const WRAPPERS: &[u32] = &[
    0x9421fff0, 0x7c0802a6, 0x38831b14, 0x90010014, 0x4bfcc821, 0x80010014, 0x7c0803a6, 0x38210010,
    0x4e800020, 0x880301c6, 0x38800001, 0x50802e34, 0x980301c6, 0x4e800020, 0x9421fff0, 0x7c0802a6,
    0x3c800000, 0x90010014, 0x38840000, 0x880301c6, 0x5400eefa, 0x7d84002e, 0x7d8903a6, 0x4e800421,
    0x80010014, 0x7c0803a6, 0x38210010, 0x4e800020,
];
const ENTRANCE: (usize, usize) = (DATA, 0x5498 + 25 * 4);
const BINDINGS: [((usize, usize), (usize, usize)); 3] = [
    ((DATA, 0x5508 + 25 * 4), (1, 0x92f68)),
    ((DATA, 0x6490), (1, 0x92f54)),
    ((DATA, 0x6494), (1, 0x92f30)),
];
pub(super) fn validate(rel: &Rel) -> Result<()> {
    ensure!(
        !rel.pointers.contains_key(&ENTRANCE) && word(rel.at(ENTRANCE)?, 0)? == 0,
        "unexpected defeat-only entrance callback"
    );
    for (address, target) in BINDINGS {
        ensure!(
            rel.pointer(address.0, address.1)? == target,
            "changed defeat-only native dispatcher"
        );
    }
    let code = rel.at((1, 0x92f30))?;
    for (index, &instruction) in WRAPPERS.iter().enumerate() {
        ensure!(
            word(code, index * 4)? == instruction,
            "changed defeat initialization or scene helper at {:#x}",
            0x92f30 + index * 4
        );
    }
    Ok(())
}

pub(super) fn duration(metadata: &ActorSettings, lengths: &VoiceDurations) -> Result<u16> {
    ensure!(
        metadata.combat.flags & 0x200000 == 0,
        "unimplemented deferred victory feedback"
    );
    super::defeat_ticks(metadata, lengths)
}

pub(super) fn policy(
    metadata: &ActorSettings,
    lengths: &VoiceDurations,
) -> Result<EnemyNativePolicy> {
    let policy = EnemyNativePolicy::DefeatPresentation {
        defeat_ticks: duration(metadata, lengths)?,
    };
    policy.validate()?;
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rel() -> Rel {
        let data = 4 + 0x92fa0;
        let mut bytes = vec![0; data + 0x6500];
        for (i, &instruction) in WRAPPERS.iter().enumerate() {
            bytes[4 + 0x92f30 + i * 4..4 + 0x92f34 + i * 4]
                .copy_from_slice(&instruction.to_be_bytes());
        }
        Rel {
            bytes,
            sections: vec![(0, 0), (4, 0x92fa0), (0, 0), (0, 0), (0, 0), (data, 0x6500)],
            pointers: BINDINGS.into(),
            local_targets: Default::default(),
        }
    }
    #[test]
    fn admission_requires_the_registered_tail_and_an_absent_entrance() {
        let mut rel = rel();
        validate(&rel).unwrap();
        for (address, target) in BINDINGS {
            rel.pointers.remove(&address);
            assert!(validate(&rel).is_err());
            rel.pointers.insert(address, target);
        }
        rel.pointers.insert(ENTRANCE, (1, 0x92f00));
        assert!(validate(&rel).is_err());
        rel.pointers.remove(&ENTRANCE);
        let at = rel.sections[DATA].0 + ENTRANCE.1;
        rel.bytes[at..at + 4].copy_from_slice(&0x92f00_u32.to_be_bytes());
        assert!(validate(&rel).is_err());
        rel.bytes[at..at + 4].fill(0);
        for offset in [0x10, 0x28, 0x30, 0x58] {
            let at = 4 + 0x92f30 + offset;
            let previous: [u8; 4] = rel.bytes[at..at + 4].try_into().unwrap();
            rel.bytes[at..at + 4].copy_from_slice(&0x60000000_u32.to_be_bytes());
            assert!(validate(&rel).is_err());
            rel.bytes[at..at + 4].copy_from_slice(&previous);
        }
    }
    #[test]
    fn defeat_uses_override_or_base_duration_without_reading_an_opening_voice() {
        let mut metadata = empty_settings();
        metadata.effects.opening_voice = 0xffff;
        metadata.effects.death_voice = 0x8002;
        let mut lengths = VoiceDurations {
            duration_ticks: vec![0, 0, 229],
        };
        assert_eq!(
            policy(&metadata, &lengths).unwrap(),
            EnemyNativePolicy::DefeatPresentation { defeat_ticks: 229 }
        );
        metadata.effects.death_voice = 0;
        for base in [0x7ffc, 0xffff_fffc] {
            metadata.effects.voice_base = base;
            assert_eq!(
                policy(&metadata, &lengths).unwrap(),
                EnemyNativePolicy::DefeatPresentation { defeat_ticks: 229 }
            );
        }
        assert!(policy(&metadata, &VoiceDurations::default()).is_err());
        lengths.duration_ticks[2] = u16::MAX;
        assert!(policy(&metadata, &lengths).is_err());
        lengths.duration_ticks[2] = 229;
        metadata.combat.flags = 0x200000;
        assert!(policy(&metadata, &lengths).is_err());
    }
    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn original_five_defeat_monitors_keep_normal_entry_and_other_action_gates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
        let lengths = VoiceDurations::original(&usual).unwrap();
        let archive = fs::read(root.join("files/BTL/BTLenemy.dat")).unwrap();
        let directory = word(&usual, 0x2c).unwrap() as usize;
        for (monster, death_ticks) in [(204, 237), (205, 79), (206, 229), (207, 125), (217, 231)] {
            let start = word(&usual, directory + monster * 4).unwrap() as usize;
            let end = word(&usual, directory + (monster + 1) * 4).unwrap() as usize;
            let bytes = compression::decode(&archive[start..end]).unwrap();
            let at = usize::from(half(&bytes, 12).unwrap());
            assert_eq!(bytes[at + 8], 25);
            let metadata =
                ActorSettings::read(&bytes[usize::from(half(&bytes, 4).unwrap())..]).unwrap();
            assert_eq!(
                native_policy(25, &rel, &metadata, &lengths).unwrap(),
                EnemyNativePolicy::DefeatPresentation {
                    defeat_ticks: death_ticks
                }
            );
        }
        assert!(
            EnemyNativePolicy::Unsupported { id: 25 }
                .validate()
                .is_err()
        );
    }
}
