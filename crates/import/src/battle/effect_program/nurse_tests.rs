use super::*;

#[test]
#[ignore = "requires the original extracted disc; validates model/animation resources without encoding"]
fn original_nurse_closes_all_four_model_controllers_without_admitting_unused_actor() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    // Full modifier interpreter, birth binding, fresh scratch initializer, and model draw.
    for (offset, size, hash) in [
        (
            0x3f0c0,
            0xac4,
            "b7e718292f08832c3e5c713b1a0229100abba9508a5f0d9633dc43dd40067bcd",
        ),
        (
            0x418b4,
            0x810,
            "fada0d5d1b343b1b6216356950ccca82f86a48c25fd859f7a52ac966bea3d661",
        ),
        (
            0x42494,
            0x158,
            "e6a07a6353a62a359ce551da0081e07658ed6be0c7130e9801259dc079b1b079",
        ),
        (
            0x3fc24,
            0x51c,
            "60f37608f2562c4f3248cf9ef2978766a5008d36550b30a0148a0dc58205ca41",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, offset)).unwrap()[..size])
            ),
            hash
        );
    }
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(37).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(package)),
        "05ccf2d9be955dda338760527d66a713daad38418f71cde6953fb2f620288c91"
    );
    let bytes = magic_member(package, 4).unwrap().unwrap();
    let bank = EffectBank::Magic(37);
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Magic(37),
        magic_member(package, 8).unwrap().unwrap().to_vec(),
    );
    for id in 1..=6 {
        cooker.program(bytes, EffectId { bank, id }).unwrap();
    }
    cooker.result.validate().unwrap();
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|actor| actor.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 2, 3, 4])
    );
    let actor = cooker.result.actor(EffectId { bank, id: 0 }).unwrap();
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(actor_source(bytes, actor.id).unwrap())
        ),
        "7c457eaec6154040b342f1b20af49ffe7fa1f18e7f1a440123245971be07e67f"
    );
    assert_eq!(actor.lifetime, Some(180));
    assert!(matches!(
        actor.geometry,
        Geometry::Model {
            model: ModelRef::Magic {
                package: 37,
                index: 0
            },
            presentation: ModelPresentation {
                external_animation: true,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        cooker.result.models_for(actor.id),
        (0..4)
            .map(|index| ModelRef::Magic { package: 37, index })
            .collect()
    );
    for index in 0u8..4 {
        let program = cooker
            .result
            .program(EffectId {
                bank,
                id: index + 2,
            })
            .unwrap();
        assert_eq!(program.end_tick, 0);
        assert_eq!(program.emissions.len(), 1);
        let emission = &program.emissions[0];
        assert_eq!(emission.tick, 0);
        assert!(emission.repeat.is_none());
        let EffectCommand::Particle {
            modifiers,
            attachment: Attachment::Emitter,
            ..
        } = &emission.command
        else {
            panic!("original one-model birth")
        };
        assert_eq!(modifiers.len(), 4);
        assert!(matches!(modifiers[0], Modifier::RequireFreshIntegers));
        assert!(
            matches!(modifiers[1],Modifier::PlayModelAnimation {animation:ModelAnimation {
            model,clip:0,blend_ticks:0,rate:0.5,hold:false}} if model==index)
        );
        assert!(
            matches!(modifiers[2],Modifier::Integer {field:IntegerField::Temporary(0),
            operation:Arithmetic::Add,value:IntegerValue::Constant(value)} if value==i16::from(index))
        );
        assert!(matches!(
            modifiers[3],
            Modifier::Byte {
                field: ByteField::ModelIndex,
                operation: Arithmetic::Set,
                value: IntegerValue::Temporary(0)
            }
        ));
        let original = vec![
            22u16,
            92,
            u16::from(index),
            0,
            0,
            0,
            0,
            0,
            3,
            0x7ffc,
            u16::from(index),
            0,
            10,
            0xd4,
            0x7ffc,
            0,
            0xffff,
            0,
        ]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
        assert_eq!(
            &bytes[1792 + usize::from(index) * 36..1828 + usize::from(index) * 36],
            original.as_slice()
        );
        // Removing the fresh-program proof must not turn Add into an implicit Set.
        assert!(actor.validate_modifiers(&modifiers[1..]).is_err());
        assert!(
            cooker
                .result
                .externally_animated(ModelRef::Magic { package: 37, index })
        );
        assert_eq!(word(package, 12 + usize::from(index) * 4).unwrap(), 135232);
        assert_eq!(
            word(package, 92 + usize::from(index) * 16).unwrap(),
            302752 + u32::from(index) * 11392
        );
    }
    // Four separate CAB bindings share one GPL. Real rig/model preflight must visit all four.
    super::super::visual::preflight_effect_models(&extracted, &cooker.result).unwrap();
    let unused = actor_source(bytes, EffectId { bank, id: 1 }).unwrap();
    assert_eq!(word(unused, 0x14).unwrap(), 0x84000000);
    assert!(
        cooker
            .actor(unused, EffectId { bank, id: 1 }, bytes, false)
            .is_err()
    );
}

#[test]
#[ignore = "requires the original extracted disc; validates Prism resources without encoding"]
fn original_prism_keeps_delayed_scratch_proof_and_zero_end_glyphs() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(32).unwrap();
    let bytes = magic_member(package, 4).unwrap().unwrap();
    let bank = EffectBank::Magic(32);
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Magic(32),
        magic_member(package, 8).unwrap().unwrap().to_vec(),
    );
    for id in 1..=10 {
        cooker.program(bytes, EffectId { bank, id }).unwrap();
    }
    cooker.result.validate().unwrap();
    let program = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
    assert_eq!(
        program.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [0, 30]
    );
    let EffectCommand::Particle {
        actor, modifiers, ..
    } = &program.emissions[1].command
    else {
        panic!("Prism delayed model birth")
    };
    assert_eq!(*actor, EffectId { bank, id: 1 });
    assert_eq!(modifiers.len(), 3);
    assert!(matches!(modifiers[0], Modifier::RequireFreshIntegers));
    assert!(matches!(
        modifiers[1],
        Modifier::Byte {
            field: ByteField::ModelIndex,
            operation: Arithmetic::Add,
            value: IntegerValue::Temporary(3)
        }
    ));
    assert!(matches!(
        modifiers[2],
        Modifier::Integer {
            field: IntegerField::Temporary(3),
            operation: Arithmetic::Add,
            value: IntegerValue::Constant(1)
        }
    ));
    assert_eq!(
        cooker.result.models_for(*actor),
        BTreeSet::from([ModelRef::Magic {
            package: 32,
            index: 4
        }])
    );
    assert!(
        cooker
            .result
            .actor(*actor)
            .unwrap()
            .validate_modifiers(&modifiers[1..])
            .is_err()
    );
    for id in 4..=10 {
        let glyph = cooker.result.program(EffectId { bank, id }).unwrap();
        assert_eq!(glyph.end_tick, 0);
        assert_eq!(glyph.emissions.len(), 1);
        let emission = &glyph.emissions[0];
        assert_eq!(emission.tick, 0);
        assert!(emission.repeat.is_none());
        assert!(
            matches!(&emission.command,EffectCommand::Particle {actor,..} if *actor==EffectId {bank,id:4})
        );
    }
    assert_eq!(
        cooker
            .result
            .actor(EffectId { bank, id: 4 })
            .unwrap()
            .lifetime,
        Some(96)
    );
    // Any earlier scratch write invalidates the delayed birth's resource proof.
    let mut changed = cooker.result.clone();
    let root = changed
        .programs
        .iter_mut()
        .find(|p| p.id == EffectId { bank, id: 1 })
        .unwrap();
    let EffectCommand::Particle { modifiers, .. } = &mut root.emissions[0].command else {
        unreachable!()
    };
    modifiers.push(Modifier::Integer {
        field: IntegerField::Temporary(3),
        operation: Arithmetic::Add,
        value: IntegerValue::Constant(1),
    });
    assert!(changed.validate().is_err());

    // The same dependency is independent of bank, program, actor, stream address,
    // delay and scratch slot. Keep the native records and relocate their bindings.
    let timeline = program_timeline(bytes, 1).unwrap();
    let events::Command::Emit { modifier, .. } =
        events::Record::read(&timeline[6..]).unwrap().command
    else {
        unreachable!()
    };
    let original_offset = usize::from(modifier);
    let relocated = original_offset + 8;
    let mut renamed = bytes.to_vec();
    // Native Set temporary2 = 0; initially dormant, then used below.
    renamed.splice(
        original_offset..original_offset,
        [0, 0, 0x7f, 0xfe, 0, 0, 0, 0],
    );
    for at in [8, 10, 12, 14, 16, 18] {
        let offset = half(bytes, at).unwrap();
        if usize::from(offset) > original_offset {
            renamed[at..at + 2].copy_from_slice(&(offset + 8).to_be_bytes());
        }
    }
    let actor_at = usize::from(half(&renamed, 8).unwrap()) + 2 * ACTOR_BYTES;
    renamed[actor_at..actor_at + ACTOR_BYTES]
        .copy_from_slice(actor_source(bytes, EffectId { bank, id: 1 }).unwrap());
    let table = usize::from(half(&renamed, 16).unwrap());
    let relative = half(&renamed, table + 2).unwrap();
    renamed[table..table + 2].copy_from_slice(&relative.to_be_bytes());
    let event_at =
        usize::try_from(i32::from(half(&renamed, 10).unwrap()) + i32::from(relative as i16))
            .unwrap();
    renamed[event_at + 6..event_at + 8].copy_from_slice(&45u16.to_be_bytes());
    renamed[event_at + 8] = 2;
    renamed[event_at + 10..event_at + 12].copy_from_slice(&(relocated as u16).to_be_bytes());
    renamed[event_at + 12..event_at + 14].copy_from_slice(&95u16.to_be_bytes());
    renamed[relocated + 4..relocated + 6].copy_from_slice(&0x7ffeu16.to_be_bytes());
    renamed[relocated + 10..relocated + 12].copy_from_slice(&0x7ffeu16.to_be_bytes());
    let renamed_root = EffectId {
        bank: EffectBank::Magic(99),
        id: 0,
    };
    let prepare = |source: &[u8]| {
        let mut cooker = test_cooker();
        cooker.textures.insert(
            TextureBank::Magic(99),
            magic_member(package, 8).unwrap().unwrap().to_vec(),
        );
        cooker.program(source, renamed_root).unwrap();
        cooker.result.validate().unwrap();
        cooker
    };
    let rebound = prepare(&renamed);
    let root = rebound.result.program(renamed_root).unwrap();
    assert_eq!(
        root.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [0, 45]
    );
    assert_eq!(root.end_tick, 95);
    let EffectCommand::Particle {
        actor, modifiers, ..
    } = &root.emissions[1].command
    else {
        unreachable!()
    };
    assert_eq!(
        *actor,
        EffectId {
            bank: renamed_root.bank,
            id: 2
        }
    );
    assert!(matches!(
        modifiers.as_slice(),
        [
            Modifier::RequireFreshIntegers,
            Modifier::Byte {
                field: ByteField::ModelIndex,
                value: IntegerValue::Temporary(2),
                ..
            },
            Modifier::Integer {
                field: IntegerField::Temporary(2),
                ..
            }
        ]
    ));
    let emission = events::Record::read(&renamed[event_at + 6..])
        .unwrap()
        .command;
    assert!(
        test_cooker()
            .particle_command_with_scratch(&renamed, renamed_root.bank, emission, false, [None; 4])
            .is_err()
    );
    let mut repeated = rebound.result.clone();
    repeated.programs[0].emissions[1].repeat = Some(Repeat {
        count: 2,
        interval: 1,
    });
    assert!(repeated.validate().is_err());

    let mut prior_write = renamed.clone();
    prior_write[event_at + 4..event_at + 6]
        .copy_from_slice(&(original_offset as u16).to_be_bytes());
    assert!(test_cooker().program(&prior_write, renamed_root).is_err());
    // Synchronous births instead have a proved inherited value, without claiming freshness.
    prior_write[event_at + 6..event_at + 8].copy_from_slice(&0u16.to_be_bytes());
    let inherited = prepare(&prior_write);
    let EffectCommand::Particle { modifiers, .. } =
        &inherited.result.program(renamed_root).unwrap().emissions[1].command
    else {
        unreachable!()
    };
    assert!(matches!(
        modifiers[0],
        Modifier::RequireIntegerRange {
            index: 2,
            min: 1,
            max: 1
        }
    ));
    assert!(
        !modifiers
            .iter()
            .any(|m| matches!(m, Modifier::RequireFreshIntegers))
    );

    // A native Set before the model read establishes its own bound and needs no
    // fresh-state assertion, even though this program would permit that proof.
    renamed[event_at + 10..event_at + 12].copy_from_slice(&(original_offset as u16).to_be_bytes());
    let initialized = prepare(&renamed);
    let EffectCommand::Particle {
        actor, modifiers, ..
    } = &initialized.result.program(renamed_root).unwrap().emissions[1].command
    else {
        unreachable!()
    };
    assert!(matches!(
        modifiers.as_slice(),
        [
            Modifier::Integer {
                operation: Arithmetic::Set,
                field: IntegerField::Temporary(2),
                value: IntegerValue::Constant(0)
            },
            ..
        ]
    ));
    assert!(
        !modifiers
            .iter()
            .any(|m| matches!(m, Modifier::RequireFreshIntegers))
    );
    assert_eq!(
        initialized.result.models_for(*actor),
        BTreeSet::from([ModelRef::Magic {
            package: 99,
            index: 4
        }])
    );
}
