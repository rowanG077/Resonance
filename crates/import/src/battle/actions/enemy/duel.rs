//! Kuchinawa disables the item command once, then uses the shared defeat monitor.
use super::*;
const CODE: &[u32] = &[
    0x9421fff0, 0x7c0802a6, 0x38831b14, 0x90010014, 0x4bfcc40d, 0x80010014, 0x7c0803a6, 0x38210010,
    0x4e800020, 0x3ca00000, 0x38800001, 0x38a50000, 0x80a50000, 0x3ca50001, 0x880559c7, 0x54000734,
    0x980559c7, 0x880301c6, 0x50802e34, 0x980301c6, 0x4e800020, 0x9421fff0, 0x7c0802a6, 0x3c800000,
    0x90010014, 0x38840000, 0x880301c6, 0x5400eefa, 0x7d84002e, 0x7d8903a6, 0x4e800421, 0x80010014,
    0x7c0803a6, 0x38210010, 0x4e800020,
];
const ENTRANCE: (usize, usize) = (DATA, 0x5498 + 27 * 4);
const BINDINGS: [((usize, usize), (usize, usize)); 3] = [
    ((DATA, 0x5508 + 27 * 4), (1, 0x93398)),
    ((DATA, 0x64a0), (1, 0x93368)),
    ((DATA, 0x64a4), (1, 0x93344)),
];
pub(super) fn validate(rel: &Rel) -> Result<()> {
    ensure!(
        !rel.pointers.contains_key(&ENTRANCE) && word(rel.at(ENTRANCE)?, 0)? == 0,
        "unexpected itemless duel entrance"
    );
    for (address, target) in BINDINGS {
        ensure!(
            rel.pointer(address.0, address.1)? == target,
            "changed itemless duel dispatcher"
        );
    }
    for (i, &instruction) in CODE.iter().enumerate() {
        ensure!(
            word(rel.at((1, 0x93344))?, i * 4)? == instruction,
            "changed itemless duel controller at {:#x}",
            0x93344 + i * 4
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn itemless_duel_requires_its_global_permission_clear_and_real_dispatch() {
        let data = 4 + 0x933d0;
        let mut rel = Rel {
            bytes: vec![0; data + 0x6500],
            sections: vec![(0, 0), (4, 0x933d0), (0, 0), (0, 0), (0, 0), (data, 0x6500)],
            pointers: BINDINGS.into(),
            local_targets: Default::default(),
        };
        for (i, word) in CODE.iter().enumerate() {
            rel.bytes[4 + 0x93344 + i * 4..4 + 0x93348 + i * 4]
                .copy_from_slice(&word.to_be_bytes());
        }
        validate(&rel).unwrap();
        for (at, target) in BINDINGS {
            rel.pointers.remove(&at);
            assert!(validate(&rel).is_err());
            rel.pointers.insert(at, target);
        }
        rel.pointers.insert(ENTRANCE, (1, 0x92f00));
        assert!(validate(&rel).is_err());
        rel.pointers.remove(&ENTRANCE);
        // Targeting a different permission bit or skipping its store must reject.
        for address in [0x93380, 0x93384, 0x93390, 0x93354] {
            let at = 4 + address;
            let old: [u8; 4] = rel.bytes[at..at + 4].try_into().unwrap();
            rel.bytes[at..at + 4].copy_from_slice(&0x60000000_u32.to_be_bytes());
            assert!(validate(&rel).is_err());
            rel.bytes[at..at + 4].copy_from_slice(&old);
        }
    }
    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn original_kuchinawa_binds_item_restriction_and_authored_defeat_duration() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
        let lengths = VoiceDurations::original(&usual).unwrap();
        let archive = fs::read(root.join("files/BTL/BTLenemy.dat")).unwrap();
        let dir = word(&usual, 0x2c).unwrap() as usize;
        let start = word(&usual, dir + 227 * 4).unwrap() as usize;
        let end = word(&usual, dir + 228 * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        assert_eq!(bytes[usize::from(half(&bytes, 12).unwrap()) + 8], 27);
        let metadata =
            ActorSettings::read(&bytes[usize::from(half(&bytes, 4).unwrap())..]).unwrap();
        assert_eq!(
            native_policy(27, &rel, &metadata, &lengths).unwrap(),
            EnemyNativePolicy::ItemlessDuel { defeat_ticks: 140 }
        );
        assert!(
            EnemyNativePolicy::Unsupported { id: 27 }
                .validate()
                .is_err()
        );
    }
}
