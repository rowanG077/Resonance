use super::*;
use resonance_battle::{Movement, Recoil, RecoilKind, VerticalRecoil};
use resonance_content::{battle_recoil, source::FloatOperand};
use resonance_game::battle::recoil::Parameters;

pub(super) fn source() -> battle_recoil::Table {
    battle_recoil::Table {
        source_sha256: "a".repeat(64),
        impulses: vec![
            [FloatOperand::Value(4.), FloatOperand::Value(-10.)],
            [FloatOperand::Value(4.), FloatOperand::Value(0.)],
        ],
        suppression_distance: FloatOperand::Value(400.),
        light_vertical_scale: FloatOperand::Value(1.15),
        heavy_vertical_scale: FloatOperand::Value(0.75),
        guard_speed: FloatOperand::Value(3.),
        default_guard_preferences: vec![0, 1, 2, 3, 5],
    }
}

fn snapshot(source: &battle_recoil::Table) -> Files {
    let mut files = files("");
    files.bytes.insert(
        battle_recoil::PATH.into(),
        serde_json::to_vec(source).unwrap().into(),
    );
    files
}

#[test]
fn preparation_requires_source_and_resolves_original_weight_and_hit_flags() -> Result<()> {
    assert!(Parameters::load(&Files::default()).is_err());
    let files = snapshot(&source());
    let parameters = Parameters::load(&files)?;
    assert_eq!(parameters.guard_recovery_bonus(0, 0)?, 10);
    assert_eq!(parameters.guard_recovery_bonus(0, 1)?, 0);
    assert_eq!(parameters.guard_recovery_bonus(0, 2)?, 5);
    assert_eq!(parameters.guard_recovery_bonus(0, 3)?, 25);
    assert_eq!(parameters.guard_recovery_bonus(0, 4)?, 10);
    assert!(parameters.guard_recovery_bonus(0, 5).is_err());
    assert_eq!(parameters.guard_recovery_bonus(3, 255)?, 25);
    let mut hit = battle::action::hit(
        &files,
        &HitResource {
            source: "test-actions.json".into(),
            selection: battle::HitSelection::Technique {
                member: 0,
                phase: 0,
            },
            rule: 0,
        },
    )?;
    hit.flags = 0x10c;
    hit.knockback_delay = 2;
    let rule = parameters.rule(&hit, 0)?;
    assert_eq!(rule.impulse, [4., -10.]);
    assert_eq!(rule.delay, 2);
    assert!(rule.knock_down && rule.launch && rule.lift_guard);
    assert!(parameters.rule(&hit, 2).is_err());
    assert_eq!(parameters.suppression_distance, 400.);
    assert_eq!(
        parameters.profile(1, 0).vertical,
        VerticalRecoil::Scale(1.15)
    );
    assert_eq!(
        parameters.profile(2, 0).vertical,
        VerticalRecoil::Scale(0.75)
    );
    assert_eq!(parameters.profile(3, 0).vertical, VerticalRecoil::Grounded);
    assert_eq!(
        parameters.profile(255, 0).vertical,
        VerticalRecoil::Unchanged
    );
    assert!(!parameters.profile(0, 0x200).can_knock_down);
    for profile in [
        parameters.profile(0, 0x200),
        parameters.profile(0, 0x02000000),
        parameters.profile(3, 0),
    ] {
        assert!(!profile.can_launch);
    }
    assert!(parameters.profile(0, 0x400).clear_pending);
    let mut invalid = source();
    invalid.impulses[0][0] = FloatOperand::Bits { bits: 0x7fc12345 };
    assert!(Parameters::load(&snapshot(&invalid)).is_err());
    invalid = source();
    invalid.guard_speed = FloatOperand::Bits { bits: 0x7f800000 };
    assert!(Parameters::load(&snapshot(&invalid)).is_err());
    Ok(())
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cold_verified_recoil_parameters_reproduce_original_opening_velocity_writes() -> Result<()> {
    let files = Files::load(
        &common::asset_root(),
        &["fields/map-340.preload.json"],
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let parameters = Parameters::load(&files)?;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-recoil.json"
    ))?;
    let source: battle_recoil::Table = files.json(battle_recoil::PATH)?;
    assert_eq!(source.source_sha256, fixture["source_module_sha256"]);
    assert_eq!(source.impulses.len(), 19);
    for (row, expected) in source
        .impulses
        .iter()
        .zip(fixture["source_parameters"]["impulses"].as_array().unwrap())
    {
        let expected: [f32; 2] = serde_json::from_value(expected.clone())?;
        assert_eq!(
            row.map(|v| v.finite().unwrap()).map(f32::to_bits),
            expected.map(f32::to_bits)
        );
    }
    for row in fixture["observations"].as_array().unwrap() {
        // The original incoming descriptor is independent of the recoil selector.
        // Its other operands remain outside this operation's acceptance scope.
        let mut hit = battle::action::hit(
            &super::files(""),
            &HitResource {
                source: "test-actions.json".into(),
                selection: battle::HitSelection::Technique {
                    member: 0,
                    phase: 0,
                },
                rule: 0,
            },
        )?;
        hit.flags = row["flags"].as_u64().unwrap() as u16;
        hit.knockback_delay = row["delay"].as_u64().unwrap() as u8;
        let rule = parameters.rule(&hit, row["selector"].as_u64().unwrap() as u8)?;
        let profile = parameters.profile(
            row["weight"].as_u64().unwrap() as u8,
            row["profile_flags"].as_u64().unwrap() as u32,
        );
        let result = row["result"].as_u64().unwrap();
        let guarded = result & 0xf == 0 || result & 0x406 != 0;
        let mut movement = Movement::default();
        let mut recoil = Recoil::default();
        assert!(recoil.start(&mut movement, rule, profile, guarded, false));
        for (actual, key) in [
            ([movement.forward, movement.vertical], "velocity_bits"),
            (recoil.pending, "pending_bits"),
        ] {
            let expected: [u32; 2] = serde_json::from_value(row["after"][key].clone())?;
            assert_eq!(actual.map(f32::to_bits), expected, "{row}");
        }
        assert_eq!(recoil.kind, RecoilKind::Normal);
        assert_eq!(recoil.delay, row["after"]["delay"].as_u64().unwrap() as u8);
    }
    let hurt: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-hurt.json"
    ))?;
    assert_eq!(source.source_sha256, hurt["source_module_sha256"]);
    for row in hurt["observations"].as_array().unwrap() {
        assert_eq!(
            parameters.guard_recovery_bonus(
                row["guard_preference"].as_u64().unwrap() as u8,
                row["actor_kind"].as_u64().unwrap() as u8,
            )?,
            row["guard_recovery_bonus"].as_u64().unwrap() as u8
        );
    }
    Ok(())
}
