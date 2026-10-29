use super::*;
use resonance_battle::{Character, VerticalRecoil};
use resonance_content::{
    battle_profile::{ProfileTraits, RecoilWeight},
    battle_recoil,
};
use resonance_game::battle::recoil::Parameters;

pub(super) fn source() -> resonance_content::battle_rules::Recoil {
    resonance_content::battle_rules::Recoil {
        impulses: (0..19)
            .map(|id| {
                (
                    id,
                    resonance_content::battle_rules::RecoilImpulse {
                        horizontal: 0.,
                        vertical: 0.,
                    },
                )
            })
            .collect(),
        suppression_distance: 0.,
        guard_speed: 3.,
        light_vertical_scale: 1.15,
        heavy_vertical_scale: 0.75,
        default_guard_recovery_bonuses: [
            ("lloyd", 0),
            ("colette", 5),
            ("genis", 25),
            ("raine", 10),
        ]
        .into_iter()
        .map(|(name, bonus)| (name.into(), bonus))
        .collect(),
    }
}

fn snapshot(source: &resonance_content::battle_rules::Recoil) -> Files {
    let mut files = files();
    files.insert(resonance_content::battle_model::document::PARTY.into(),br#"{"1":"lloyd","2":"colette","3":"genis","4":"raine","5":"sheena","6":"zelos","7":"presea","8":"regal","9":"kratos"}"#.to_vec().into());
    files.insert(
        battle_recoil::PATH.into(),
        serde_json::to_vec(source).unwrap().into(),
    );
    files
}

#[test]
fn preparation_requires_assets_and_resolves_weight_and_guard_preferences() -> Result<()> {
    assert!(Parameters::load(&Files::default()).is_err());
    let files = snapshot(&source());
    let parameters = Parameters::load(&files)?;
    for (character, bonus) in [
        (Character::Lloyd, 0),
        (Character::Colette, 5),
        (Character::Genis, 25),
        (Character::Raine, 10),
    ] {
        assert_eq!(parameters.guard_recovery_bonus(0, character)?, bonus);
    }
    assert!(
        parameters
            .guard_recovery_bonus(0, Character::Sheena)
            .is_err()
    );
    assert_eq!(parameters.guard_recovery_bonus(3, Character::Sheena)?, 25);
    let ordinary = ProfileTraits::default();
    for (weight, vertical) in [
        (RecoilWeight::Light, VerticalRecoil::Scale(1.15)),
        (RecoilWeight::Heavy, VerticalRecoil::Scale(0.75)),
        (RecoilWeight::Grounded, VerticalRecoil::Grounded),
        (RecoilWeight::Normal, VerticalRecoil::Unchanged),
    ] {
        assert_eq!(parameters.profile(weight, &ordinary).vertical, vertical);
    }
    let immovable = ProfileTraits {
        knockdown_immune: true,
        ..ordinary
    };
    let grounded = ProfileTraits {
        launch_immune: true,
        ..ordinary
    };
    let interruptible = ProfileTraits {
        clear_pending_on_hit: true,
        ..ordinary
    };
    let immovable = parameters.profile(RecoilWeight::Normal, &immovable);
    assert!(!immovable.can_knock_down && !immovable.can_launch);
    assert!(
        !parameters
            .profile(RecoilWeight::Normal, &grounded)
            .can_launch
    );
    assert!(
        !parameters
            .profile(RecoilWeight::Grounded, &ordinary)
            .can_launch
    );
    assert!(
        parameters
            .profile(RecoilWeight::Normal, &interruptible)
            .clear_pending
    );
    Ok(())
}
