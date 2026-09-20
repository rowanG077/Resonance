//! Parse both full Wind effect closures without encoding or publishing cooked assets.
use super::*;

#[test]
#[ignore = "requires privately extracted US assets; parses original effects, palettes and models"]
fn original_stored_wind_closes_spirals_models_trails_and_the_empty_air_blade_initializer() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let mut cooker = test_cooker();
    for (package, count, model_count) in [(10, 2, 1), (11, 4, 2)] {
        let source = archive.package(package).unwrap();
        let bytes = magic_member(source, 4).unwrap().unwrap();
        let bank = EffectBank::Magic(package);
        assert_eq!(bytes[4], count);
        cooker.textures.insert(
            TextureBank::Magic(package),
            magic_member(source, 8).unwrap().unwrap().to_vec(),
        );
        for id in 1..count {
            cooker.program(bytes, EffectId { bank, id }).unwrap();
        }
        let models = cooker
            .result
            .models()
            .filter_map(|model| match model {
                ModelRef::Magic {
                    package: actual,
                    index,
                } if actual == package => Some(index),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(models, (0..model_count).collect());
        for index in models {
            crate::model_preview::preflight(crate::model_preview::Layer {
                model: magic_member(source, 12 + usize::from(index) * 4)
                    .unwrap()
                    .unwrap(),
                outline: magic_member(source, 52 + usize::from(index) * 4).unwrap(),
                animation: None,
                attached_to: None,
                additive: false,
            })
            .unwrap();
        }
        let initial = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
        if package == 10 {
            assert_eq!((initial.end_tick, initial.emissions.len()), (180, 9));
            assert_eq!(
                initial.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
                [10, 10, 20, 30, 40, 40, 45, 45, 60]
            );
        } else {
            assert!(initial.emissions.is_empty());
            assert_eq!(initial.end_tick, 0);
            let models = cooker.result.program(EffectId { bank, id: 2 }).unwrap();
            assert_eq!((models.end_tick, models.emissions.len()), (0, 2));
            let trails = cooker.result.program(EffectId { bank, id: 3 }).unwrap();
            assert_eq!((trails.end_tick, trails.emissions.len()), (1, 6));
        }
    }
    cooker.result.validate().unwrap();
}
