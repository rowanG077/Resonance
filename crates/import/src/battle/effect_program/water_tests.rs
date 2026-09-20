//! Complete water particle/model closure; encoding and output writes are unnecessary here.
use super::*;

#[test]
#[ignore = "requires privately extracted US assets; parses original effects, palettes and models"]
fn original_stored_water_closes_models_spray_and_empty_initializer() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let mut cooker = test_cooker();
    for (package, count) in [(2, 2), (3, 4)] {
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
        assert_eq!(models, (0..if package == 2 { 6 } else { 3 }).collect());
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
        if package == 2 {
            assert_eq!(initial.end_tick, 160);
            assert!(matches!(
                initial.emissions[0].command,
                EffectCommand::Sound {
                    sound: 169,
                    priority: 0
                }
            ));
            assert!(matches!(
                initial.emissions[0].repeat,
                Some(Repeat {
                    count: 8,
                    interval: 15
                })
            ));
        } else {
            assert!(initial.emissions.is_empty());
            assert_eq!(initial.end_tick, 0);
            let models = cooker.result.program(EffectId { bank, id: 2 }).unwrap();
            assert_eq!(models.emissions.len(), 3);
            let spray = cooker.result.program(EffectId { bank, id: 3 }).unwrap();
            assert_eq!((spray.end_tick, spray.emissions.len()), (1, 5));
        }
    }
    cooker.result.validate().unwrap();
}
