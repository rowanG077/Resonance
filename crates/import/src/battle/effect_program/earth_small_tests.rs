//! Complete Stone Blast and Stalagmite effects, including retained model modifiers.
use super::*;

#[test]
#[ignore = "requires original US effects, palettes and model sources; no encoding"]
fn original_stone_and_stalagmite_close_all_actor_and_model_resources() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Fixed(0),
        compression::decode(member(member(&usual, 4).unwrap(), 1).unwrap()).unwrap(),
    );
    let stone = EffectId {
        bank: EffectBank::Techniques,
        id: 21,
    };
    cooker.program(member(&usual, 3).unwrap(), stone).unwrap();
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([24, 25, 26, 27])
    );
    assert!(
        cooker
            .result
            .models()
            .any(|m| m == ModelRef::Common { index: 3 })
    );
    let common = member(&usual, 6).unwrap();
    crate::model_preview::preflight(crate::model_preview::Layer {
        model: member(common, 2).unwrap(),
        outline: None,
        animation: None,
        attached_to: None,
        additive: false,
    })
    .unwrap();
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = archive.package(13).unwrap();
    cooker.textures.insert(
        TextureBank::Magic(13),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    let stalagmite = EffectId {
        bank: EffectBank::Magic(13),
        id: 1,
    };
    cooker
        .program(magic_member(source, 4).unwrap().unwrap(), stalagmite)
        .unwrap();
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .filter(|a| a.id.bank == stalagmite.bank)
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        (0..10).collect()
    );
    // The remaining three source actors are shake controllers, not particles.
    let root = cooker.result.program(stalagmite).unwrap();
    assert_eq!(
        root.emissions
            .iter()
            .filter_map(|emission| match &emission.command {
                EffectCommand::Controller { actor, controller } => {
                    assert_eq!(actor.bank, stalagmite.bank);
                    assert!(matches!(controller, EffectController::Shake { .. }));
                    Some((emission.tick, actor.id))
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(0, 12), (50, 10), (66, 10), (100, 10), (124, 11)]
    );
    assert_eq!(
        cooker
            .result
            .models()
            .filter(|m| matches!(m, ModelRef::Magic { package: 13, .. }))
            .collect::<BTreeSet<_>>(),
        (0..3)
            .map(|index| ModelRef::Magic { package: 13, index })
            .collect()
    );
    crate::battle::visual::preflight_effect_models(&extracted, &cooker.result).unwrap();
    cooker.result.validate().unwrap();
}
