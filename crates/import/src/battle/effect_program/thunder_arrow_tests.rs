//! Both source-native visual roots must retain their full actor and modifier closure.
use super::*;

#[test]
#[ignore = "requires original US Magic27 effects and palettes; no encoding"]
fn original_thunder_arrow_closes_both_visual_roots_and_all_palettes() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = archive.package(27).unwrap();
    let bytes = magic_member(source, 4).unwrap().unwrap();
    assert_eq!(bytes[4], 3);
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Magic(27),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    for id in [1, 2] {
        cooker
            .program(
                bytes,
                EffectId {
                    bank: EffectBank::Magic(27),
                    id,
                },
            )
            .unwrap();
    }
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        (2..10).collect()
    );
    assert_eq!(cooker.result.models().count(), 0);
    assert!((0..10).all(|index| magic_member(source, 12 + index * 4).unwrap().is_none()));
    let initial = cooker
        .result
        .program(EffectId {
            bank: EffectBank::Magic(27),
            id: 1,
        })
        .unwrap();
    let satellite = cooker
        .result
        .program(EffectId {
            bank: EffectBank::Magic(27),
            id: 2,
        })
        .unwrap();
    assert_eq!((initial.end_tick, satellite.end_tick), (140, 150));
    assert!(
        initial
            .emissions
            .iter()
            .any(|e| matches!(e.command, EffectCommand::Sound { sound: 92, .. }))
    );
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .filter(|a| matches!(a.geometry, Geometry::JitterRibbon { .. }))
            .count(),
        2
    );
    assert_eq!(
        initial
            .emissions
            .iter()
            .filter_map(|e| match e.command {
                EffectCommand::Controller { actor, .. } => Some(actor.id),
                _ => None,
            })
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 1])
    );
    cooker.result.validate().unwrap();

    // The six surrounding ribbon variants cannot disappear when one operation is unknown.
    let mut unknown = bytes.to_vec();
    unknown[3552] = 254;
    let mut invalid = test_cooker();
    invalid.textures.insert(
        TextureBank::Magic(27),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    assert!(
        invalid
            .program(
                &unknown,
                EffectId {
                    bank: EffectBank::Magic(27),
                    id: 1
                }
            )
            .is_err()
    );
}
