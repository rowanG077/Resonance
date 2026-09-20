//! Acid Rain has a stored scene and one status pulse, with no damage/projectile program.
use super::*;
use resonance_content::battle::{actions::acid_rain::AcidRainRecipe, conditions::StatDebuff};
use sha2::{Digest, Sha256};

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    row: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    ensure!(
        row.native_id as u16 == 265 && row.flags == 0x00840087,
        "unexpected Acid Rain identity or flags"
    );
    for character in 1..=9 {
        let learned = catalogue.learned_by(character)?;
        ensure!(
            !learned.iter().any(|&id| u16::from(id) == technique),
            "Acid Rain party casting binding requires recovery"
        );
    }
    let recipe = tables.elemental.acid_rain;
    recipe.validate()?;
    Ok(TechniqueProgram::AcidRain { recipe })
}

pub(super) fn read_parameters(rel: &Rel) -> Result<AcidRainRecipe> {
    let dispatch = rel.pointer(DATA, 0x1238 + 65 * 4)?;
    for (slot, function) in [(0, 0x7667c), (4, 0x37e48), (8, 0x37dd8)] {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + slot)? == (1, function),
            "unexpected Acid Rain dispatch"
        );
    }
    // Guard the opposing-roster predicate, signed status arguments and global scene origin.
    for (offset, size, digest) in [
        (
            0x76590,
            0xec,
            "3bd61f95e52b3bea30a37de88a6937bfb5cc260ce43f2dbb3a1c49bc4845bc9b",
        ),
        (
            0x7667c,
            0xf8,
            "0152e2ed6959921094475547057affa2d30d6a7305cdd261db2a27b7d9af217a",
        ),
    ] {
        let body = rel
            .at((1, offset))?
            .get(..size)
            .context("truncated Acid Rain controller")?;
        ensure!(
            format!("{:x}", Sha256::digest(body)) == digest,
            "changed Acid Rain controller at {offset:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x76590)),
        "missing Acid Rain active callback"
    );
    let operand = |address: usize| half(rel.at((1, address + 2))?, 0);
    let settings = rel.at((4, 0x54f0))?;
    let tint = usize::from(operand(0x7662c)?);
    let recipe = AcidRainRecipe {
        lifetime: operand(0x766cc)?,
        application_tick: operand(0x765b4)?,
        effect: AcidRainRecipe::effect(operand(0x76708)?.try_into()?),
        origin: [float(settings, 12)?; 3],
        effect_scale: float(settings, 16)?,
        stat: StatDebuff::DefenseDown,
        amount: operand(0x76614)? as i16,
        duration: operand(0x76618)? as i16,
        retained: operand(0x76620)? != 0,
        tint: rel.at((4, 0x1564 + tint * 4))?[..4].try_into()?,
        presentation: StoredSpellPresentation {
            color: settings[..4].try_into()?,
            camera_distance: float(settings, 4)?,
            camera_elevation: float(settings, 8)?,
        },
    };
    recipe.validate()?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::effect_program::{MagicArchive, magic_member};
    use resonance_content::battle::effects::{EffectBank, EffectId};

    #[test]
    #[ignore = "requires original US disc; parses Acid Rain and complete owner bindings without encoding"]
    fn original_acid_rain_keeps_status_only_scene_and_enemy_casting_ownership() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let technique = technique_actions(&extracted, &rel, &usual, &[226])
            .unwrap()
            .remove(0);
        assert_eq!(technique.enemy_spell(), Some(EnemySpell::AcidRain));
        let TechniqueProgram::AcidRain { recipe } = &technique.program else {
            panic!("Acid Rain recipe")
        };
        assert_eq!(
            (
                recipe.lifetime,
                recipe.application_tick,
                recipe.amount,
                recipe.duration,
                recipe.retained
            ),
            (240, 120, 15, 0, true)
        );
        assert_eq!(
            (recipe.origin, recipe.effect_scale, recipe.tint),
            ([0.; 3], 1., [32, 32, 32, 255])
        );
        assert_eq!(recipe.presentation.color, [48, 32, 32, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2400., 8.)
        );
        assert!(technique.program.stored().unwrap().party_resume.is_none());
        assert_eq!(technique.program.spell_rules().count(), 0);
        let actions = BattleActions {
            party: vec![],
            enemies: vec![],
            projectiles: vec![],
            techniques: vec![technique],
            chains: None,
        };
        actions.validate().unwrap();
        let dependencies = crate::battle::selection::Dependencies::actions(&actions).unwrap();
        assert!(dependencies.projectiles.is_empty());
        for effect in [
            EffectId {
                bank: EffectBank::Common,
                id: 37,
            },
            AcidRainRecipe::effect(1),
            AcidRainRecipe::effect(2),
        ] {
            assert!(dependencies.programs.contains(&effect));
        }
        let (sounds, voices) = actions.audio_ids();
        assert!(sounds.contains(&122));
        assert!(voices.is_empty());
        let magic = MagicArchive::read(&extracted).unwrap();
        let package = magic.package(65).unwrap();
        assert!(magic_member(package, 252).unwrap().is_none());

        let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        for monster in [56u8, 186] {
            let start = word(&usual, table + usize::from(monster) * 4).unwrap() as usize;
            let end = word(&usual, table + (usize::from(monster) + 1) * 4).unwrap() as usize;
            let package = compression::decode(&archive[start..end]).unwrap();
            let enemy = enemy_actions(&package, monster).unwrap();
            let casting = enemy::casting(&package, &catalogue, &usual, &rel).unwrap();
            let acid = enemy
                .actions
                .iter()
                .filter(|action| action.technique == Some(265))
                .collect::<Vec<_>>();
            assert!(
                !acid.is_empty(),
                "missing original Acid Rain owner {monster}"
            );
            for action in acid {
                let cast = casting
                    .get(&action.id)
                    .expect("original enemy Acid Rain casting");
                assert!(cast.duration > 0 && cast.tp > 0);
            }
        }
    }
}
