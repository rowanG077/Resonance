use super::*;

#[test]
#[ignore = "requires original extracted GameCube assets; no texture encoding or output"]
fn original_common_contact_closure_decodes_guard_break_repeated_guard_and_inert_critical() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let bank = member(&usual, 2).unwrap();
    let textures = member(&usual, 4).unwrap();
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let table = super::super::contact_effects::cook(&rel).unwrap();
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Fixed(0),
        compression::decode(member(textures, 1).unwrap()).unwrap(),
    );
    cooker
        .textures
        .insert(TextureBank::Fixed(1), member(textures, 4).unwrap().to_vec());
    cooker.element_palettes = rel.at((4, 0x2174)).unwrap()[1..9].try_into().unwrap();
    let colors = rel.at((4, 0x2180)).unwrap();
    cooker.element_colors =
        std::array::from_fn(|i| std::array::from_fn(|c| i16::from(colors[(i + 1) * 4 + c])));
    for id in table.programs() {
        cooker.program(bank, id).unwrap();
    }
    cooker.result.validate().unwrap();
    let program = |id| {
        cooker
            .result
            .program(EffectId {
                bank: EffectBank::Common,
                id,
            })
            .unwrap()
    };
    let broken = program(0);
    assert_eq!(broken.end_tick, 8);
    assert_eq!(
        broken.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [0, 1, 1]
    );
    assert_eq!(
        broken
            .emissions
            .iter()
            .map(|e| e.repeat.map(|r| (r.count, r.interval)))
            .collect::<Vec<_>>(),
        [Some((3, 2)), Some((3, 2)), None]
    );
    assert_eq!(
        broken
            .emissions
            .iter()
            .map(|e| match e.command {
                EffectCommand::Particle {
                    actor,
                    attachment: Attachment::Emitter,
                    ..
                } => actor.id,
                _ => panic!(),
            })
            .collect::<Vec<_>>(),
        [16, 16, 17]
    );
    let recent = program(2);
    assert_eq!(recent.end_tick, 8);
    assert_eq!(recent.emissions.len(), 1);
    assert_eq!(recent.emissions[0].tick, 2);
    assert!(matches!(
        recent.emissions[0].command,
        EffectCommand::Particle {
            actor: EffectId {
                bank: EffectBank::Common,
                id: 8
            },
            attachment: Attachment::Emitter,
            ..
        }
    ));
    let critical = program(16);
    assert_eq!(critical.end_tick, 8);
    assert!(
        critical.emissions.is_empty(),
        "original critical program is an inert timeline, not a missing resource"
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(
                actor_source(
                    bank,
                    EffectId {
                        bank: EffectBank::Common,
                        id: 16
                    }
                )
                .unwrap()
            )
        ),
        "97eedd4e3424b1898aa85bb41e4fb127393df4d1b0da87c896a9e781cc770114"
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(
                actor_source(
                    bank,
                    EffectId {
                        bank: EffectBank::Common,
                        id: 17
                    }
                )
                .unwrap()
            )
        ),
        "17444a0d3b667a6814679d5d81ab12eed9d0ae37dfa76396c9ce680da1a57b09"
    );
}
