//! Preserve Falcon's Crest native gameplay and publish its stored presentation.
use super::*;
use resonance_content::{
    battle_martial::{AuthoredAction, Presentation},
    battle_unison::Placement,
};

const ACTION: &str = "battle/artes/falcons-crest/action.json";

pub(super) fn action_path(image: &pe::Image<'_>, row: &[u8]) -> Result<String> {
    ensure!(
        pe::half(row, 0)? == 140,
        "Falcon's Crest native member changed"
    );
    handlers(image)?;
    Ok(ACTION.to_owned())
}

fn handlers(image: &pe::Image<'_>) -> Result<()> {
    let address = pe::word(image.slice(0x8e98d0 + 140 * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(address, 4)?, 0)? == 0x40c5d0
            && pe::word(image.slice(address + 4, 4)?, 0)? == 0x427f20,
        "Falcon's Crest native handlers changed"
    );
    ensure!(
        image.slice(0x40c6ac, 7)? == [0xc7, 0x46, 0x18, 0x10, 0xc9, 0x40, 0],
        "Falcon's Crest callback changed"
    );
    Ok(())
}

pub(super) fn attach(source: &Source, image: &pe::Image<'_>, attack: &mut Attack) -> Result<()> {
    handlers(image)?;
    ensure!(
        attack.duration == 350,
        "Falcon's Crest native duration changed"
    );
    let slot = pe::word(image.slice(0x8d84b0 + 228 * 96 + 0x58, 4)?, 0)? + 1;
    let name = image.text(pe::word(image.slice(0x8e98a8 + slot * 4, 4)?, 0)?)?;
    let package = source.require(name)?;
    let rows = crate::battle_scenes::member(&package, 0x18c)?
        .context("missing Falcon's Crest projectiles")?;
    let reset = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    for (index, (at, member, address, expected)) in [
        (
            16,
            1,
            0x40cab0,
            &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 0x10][..],
        ),
        (
            90,
            2,
            0x40cb30,
            &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 0x5a][..],
        ),
        (210, 3, 0x40cbb3, &[0xba, 0xd2, 0, 0, 0][..]),
        (256, 4, 0x40cc45, &[0xba, 0, 1, 0, 0][..]),
    ]
    .into_iter()
    .enumerate()
    {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "Falcon's Crest projectile clock changed"
        );
        let row = rows
            .get(member * 400..(member + 1) * 400)
            .context("missing Falcon's Crest projectile")?;
        let mut definition = resonance_importer::decode_projectile(
            &crate::battle_projectiles::canonical_record(row)?,
            reset,
        )?;
        ensure!(
            definition.unsupported_reason.is_none(),
            "unsupported Falcon's Crest projectile"
        );
        for effect in [
            &mut definition.birth_effect,
            &mut definition.trail_effect,
            &mut definition.ground_effect,
        ] {
            if effect.member != 0 {
                ensure!(
                    matches!(effect.bank, 0 | 6 | 8),
                    "unknown Falcon's Crest effect bank"
                );
                effect.bank = 2;
            }
        }
        let mut hit = attack
            .callback_hits
            .remove(&(index as u8))
            .context("missing Falcon's Crest callback hit")?;
        hit.recoil = row[19];
        attack.events.push((
            at,
            Event::EmitProjectile {
                definition: Box::new(definition),
                hit,
                feedback: None,
            },
        ));
    }
    attack.chain_at = None;
    // Initializer40c6c4 retains transition protection for340 source ticks.
    ensure!(
        image.slice(0x40c6c4, 5)? == [0xb8, 0x54, 1, 0, 0],
        "Falcon's Crest protection changed"
    );
    attack.events.push((0, Event::TransitionProtection(340)));
    ensure!(
        image.slice(0x40c8e4, 5)? == [0x68, 0x18, 1, 0, 0]
            && image.slice(0x40c8eb, 5)? == [0xe8, 0x20, 0x6a, 5, 0],
        "Falcon's Crest scene admission changed"
    );
    attack.events.push((
        0,
        Event::ActionScene {
            party_protection: 280,
        },
    ));
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}

fn presentation(image: &pe::Image<'_>) -> Result<Vec<(u16, Presentation)>> {
    // Native initializer: stage310, camera280. Native callback: weapon10/270,
    // portrait style5 at166. UI451110 advances that style for44 source ticks.
    for (address, expected) in [
        (0x40c608, &[0xba, 0x36, 1, 0, 0][..]),
        (0x40c684, &[0xba, 0x18, 1, 0, 0][..]),
        (0x40c93a, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 0x0a][..]),
        (0x40c970, &[0xb8, 0xa6, 0, 0, 0][..]),
        (0x40ca19, &[0xba, 0x0e, 1, 0, 0][..]),
        (0x451137, &[0x83, 0xf8, 0x14][..]),
        (0x451180, &[0x66, 0x83, 0x81, 0x12, 0x8d, 0, 0, 0xe7][..]),
    ] {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "Falcon's Crest presentation changed"
        );
    }
    let single =
        |address| -> Result<f32> { Ok(f32::from_le_bytes(image.slice(address, 4)?.try_into()?)) };
    let effect = |member, bone: Option<&str>| Presentation::SkillEffect {
        member,
        bone: bone.map(str::to_owned),
    };
    Ok(vec![
        (
            0,
            Presentation::StageTint {
                color: [0; 3],
                duration: 310,
            },
        ),
        (
            0,
            Presentation::Camera {
                bounds: resonance_content::battle_scene::CameraBounds {
                    minimum_distance: single(0x883f70)?,
                    minimum_elevation: single(0x884054)?,
                },
                duration: 280,
            },
        ),
        (0, effect(1, None)),
        (10, Presentation::EquipSkillWeapon),
        (
            10,
            Presentation::WeaponVisibility {
                slot: resonance_content::battle_action::WeaponSlot(1),
                visible: false,
            },
        ),
        (10, effect(2, Some("kk00"))),
        (166, Presentation::Cutin { duration: 44 }),
        (270, effect(2, Some("kk00"))),
        (270, effect(2, Some("kk01"))),
        (270, Presentation::RestoreWeapons),
    ])
}

pub(super) fn cook(source: &Source, library: &Path, catalogue: &Catalogue) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let row = image.slice(0x8d84b0 + 228 * 96, 96)?;
    let path = action_path(&image, row)?;
    output::json(
        &library.join(path),
        &AuthoredAction {
            attack: catalogue.attack(228, 1)?.clone(),
            motions: BTreeMap::new(),
            effects: None,
            presentation: presentation(&image)?,
            unison: Placement {
                native_id: 140,
                altitude: pe::word(row, 0x50)?,
                distance: pe::half(row, 0x54)?,
                duration: pe::half(row, 0x56)?,
            },
        },
    )
}
