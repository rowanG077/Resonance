use super::*;
use crate::battle::effect_program::{MagicArchive, magic_member};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires original US disc; parses source records without encoding assets"]
fn original_nurse_binds_roster_models_and_meredys_delayed_group_recovery() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let catalogue = crate::arte::read(&executable).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let tables = Tables::original(&extracted, &rel, &usual, &[]).unwrap();
    for (offset, length, hash) in [
        (
            0x60510,
            0x184,
            "18f6771028ac55aee0f5bb55f8aad99d05a612049458bba3baa1130c67898050",
        ),
        (
            0x60694,
            0x1d8,
            "450d7a3e2eff4d8c13da4cdde8cdf8d4352e62928d9d86a9b80131bc31a55334",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, offset)).unwrap()[..length])
            ),
            hash
        );
    }
    let row = catalogue.definition(99).unwrap();
    let program = cook(&catalogue, &tables, 99, row).unwrap();
    let TechniqueProgram::Nurse {
        casters,
        recipe,
        resume,
    } = &program
    else {
        panic!("Nurse recipe")
    };
    assert_eq!(
        (recipe.lifetime, recipe.recovery_tick, recipe.percent),
        (250, 120, 40)
    );
    assert_eq!((recipe.first_heading, recipe.heading_step), (15., 90.));
    assert_eq!(recipe.presentation.color, [16, 16, 16, 255]);
    assert_eq!(
        (
            recipe.presentation.camera_distance,
            recipe.presentation.camera_elevation
        ),
        (1950., 8.)
    );
    assert_eq!(casters.len(), 1);
    assert_eq!(
        (
            casters[0].character,
            casters[0].tp,
            casters[0].time_adjustment
        ),
        (4, 28, 240)
    );
    assert!(matches!(
        resume,
        AnimationCommand::Play {
            clip: 12,
            start: 0,
            rate: 0.5,
            ..
        }
    ));
    assert!(matches!(
        casters[0].release,
        Some(AnimationCommand::Play {
            clip: 13,
            blend: 4,
            ..
        })
    ));

    let action = TechniqueAction {
        technique: 99,
        native_id: 237,
        properties: technique_properties(row).unwrap(),
        program: program.clone(),
    };
    assert_eq!(action.enemy_spell(), Some(EnemySpell::Nurse));
    let actions = BattleActions {
        party: vec![],
        enemies: vec![],
        projectiles: vec![],
        techniques: vec![action],
        chains: None,
    };
    let dependencies = crate::battle::selection::Dependencies::actions(&actions).unwrap();
    assert_eq!(
        dependencies
            .programs
            .iter()
            .copied()
            .filter(|id| id.bank == resonance_content::battle::effects::EffectBank::Magic(37))
            .collect::<Vec<_>>(),
        (1..=6).map(NurseRecipe::effect).collect::<Vec<_>>()
    );
    assert!(dependencies.projectiles.is_empty());

    let magic = MagicArchive::read(&extracted).unwrap();
    let package = magic.package(37).unwrap();
    assert_eq!(
        word(package, 252).unwrap(),
        0,
        "Nurse has no projectile bank"
    );
    let bank = magic_member(package, 4).unwrap().unwrap();
    let logical = &bank[..usize::from(half(bank, 18).unwrap()) + usize::from(bank[5]) * 2];
    assert_eq!(
        format!("{:x}", Sha256::digest(logical)),
        "2475a0f869da640f4f14627f9c95075bdb64d78091498a8c97820cad3a19c735"
    );
    assert_eq!(bank[4], 7);
    let mut referenced = Vec::new();
    for id in 0..7 {
        let mut at = usize::from(half(bank, 10).unwrap())
            .checked_add_signed(isize::from(
                half(bank, usize::from(half(bank, 16).unwrap()) + id * 2).unwrap() as i16,
            ))
            .unwrap();
        let mut actors = Vec::new();
        for _ in 0..32 {
            let opcode = bank[at + 2];
            if opcode == 254 {
                break;
            }
            if opcode < 250 {
                actors.push(opcode);
            }
            at += 6;
        }
        referenced.push(actors);
    }
    assert_eq!(
        referenced,
        [
            vec![],
            vec![],
            vec![0],
            vec![0],
            vec![0],
            vec![0],
            vec![3, 2, 4]
        ]
    );
    // The inaccessible high-flag actor remains in the source; no Nurse dependency references it.
    assert_eq!(
        word(bank, usize::from(half(bank, 8).unwrap()) + 352 + 0x14).unwrap(),
        0x84000000
    );
    for i in 0..4 {
        let modifier = 1792 + i * 36;
        assert_eq!(half(bank, modifier).unwrap(), 22);
        assert_eq!(half(bank, modifier + 4).unwrap(), i as u16);
        assert_eq!(
            half(bank, modifier + 16).unwrap(),
            3,
            "model index uses authored Add, not Set"
        );
        assert_eq!(half(bank, modifier + 20).unwrap(), i as u16);
        assert_eq!(half(bank, modifier + 24).unwrap(), 10);
        assert_eq!(half(bank, modifier + 26).unwrap(), 0xd4);
    }

    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let start = word(&usual, table + 233 * 4).unwrap() as usize;
    let end = word(&usual, table + 234 * 4).unwrap() as usize;
    let enemy = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&archive[start..end])),
        "ca9cad5100378c2f0213a8f14815a6b4592d7924ff3f4d1dd28988cc4e845a2a"
    );
    let action = usize::from(half(&enemy, 10).unwrap()) + 12 * ACTION_BYTES;
    assert_eq!(action, 1888);
    assert_eq!(half(&enemy, action + 0x40).unwrap(), 237);
    assert_eq!(
        (
            half(&enemy, action + 0x3c).unwrap(),
            half(&enemy, action + 0x3e).unwrap()
        ),
        (0, 34654)
    );
    let casting = super::super::enemy::casting(&enemy, &catalogue, &usual, &rel).unwrap();
    assert_eq!(casting[&12].voices.release, 34654);
    assert!(EnemySpell::Nurse.stored());
    assert!(matches!(
        casting[&12].release,
        AnimationCommand::Play { clip: 13, .. }
    ));
    let callback = rel.sections[1].0 + 0x60528;
    rel.bytes[callback + 3] = 119;
    assert!(
        read_parameters(&rel).is_err(),
        "changed callback cannot retain the fixed recovery schedule"
    );
}
