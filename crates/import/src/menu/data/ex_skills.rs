//! Resolve skill definitions and character-specific compound recipes into JSON.
use super::*;
use crate::read::{u16 as half, u32 as word};
use resonance_content::menu_data::{
    CharacterExSkills, CompoundExSkill, ExActivation, ExSkill, ExSkillData, ExStat, ExStatBonus,
    ExTendency,
};
use std::collections::BTreeSet;

pub(super) fn cook(executable: &[u8]) -> Result<ExSkillData> {
    const CHOICES: u32 = 0x80208dd0;
    const COMPOUNDS: u32 = 0x80208e60;
    const SKILLS: u32 = 0x80209544;
    const LABELS: u32 = 0x801ab080;
    const SKILL_COUNT: usize = 166;

    // Decode the save-point cost override into a skill rule, not executable bytes.
    let immediate = |address, opcode| -> Result<u16> {
        let instruction = word(dol::slice(executable, address, 4)?, 0)?;
        ensure!(
            instruction & 0xffff_0000 == opcode,
            "unsupported save-point TP rule"
        );
        Ok(instruction as u16)
    };
    let caster = usize::from(immediate(0x800cf788, 0x2c00_0000)?);
    ensure!(caster < 9, "invalid save-point TP caster");
    let save_point_skill = half(dol::slice(executable, 0x8018d1f0, 18)?, caster * 2)?;
    let save_point_cost: u8 = immediate(0x800cf7a0, 0x3860_0000)?.try_into()?;
    let choices = dol::slice(executable, CHOICES, 9 * 16)?;
    let characters: Vec<_> = dol::slice(executable, COMPOUNDS, 9 * 196)?
        .chunks_exact(196)
        .zip(choices.chunks_exact(16))
        .map(|(row, choices)| {
            ensure!(word(row, 0)? == 24, "invalid compound EX skill count");
            Ok(CharacterExSkills {
                levels: std::array::from_fn(|i| choices[i * 4..i * 4 + 4].try_into().unwrap()),
                compounds: row[4..]
                    .chunks_exact(8)
                    .map(|r| {
                        let count = usize::from(half(r, 2)?);
                        ensure!(
                            (2..=4).contains(&count),
                            "invalid compound EX skill requirements"
                        );
                        ensure!(
                            r[4 + count..].iter().all(|&v| v == 0),
                            "unexpected compound EX skill padding"
                        );
                        Ok(CompoundExSkill {
                            skill: half(r, 0)?.try_into()?,
                            required: r[4..4 + count].to_vec(),
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    ensure!(
        characters.iter().enumerate().all(|(i, character)| character
            .levels
            .iter()
            .flatten()
            .any(|&id| u16::from(id) == save_point_skill)
            == (i == caster)),
        "save-point TP skill does not match its caster"
    );
    // Unreferenced development placeholders have no gameplay identity to deploy.
    let referenced: BTreeSet<_> = characters
        .iter()
        .flat_map(|c| {
            c.levels
                .iter()
                .flatten()
                .copied()
                .chain(c.compounds.iter().map(|c| c.skill))
        })
        .collect();
    let rows = dol::slice(executable, SKILLS, SKILL_COUNT * 20)?;
    let skills = referenced
        .into_iter()
        .map(|id| {
            let at = usize::from(id) * 20;
            let row = rows
                .get(at..at + 20)
                .context("EX skill reference outside definitions")?;
            ensure!(
                word(row, 0)? == u32::from(id) && row[18] == 0,
                "unsupported EX skill definition {id}"
            );
            let stat_bonuses = row[12..16]
                .chunks_exact(2)
                .filter(|r| r[0] != 0 || r[1] != 0)
                .map(|r| {
                    let stat = match r[0] {
                        1 => ExStat::Strength,
                        2 => ExStat::Defense,
                        3 => ExStat::Accuracy,
                        4 => ExStat::Evasion,
                        5 => ExStat::MaxHp,
                        6 => ExStat::MaxTp,
                        7 => ExStat::Luck,
                        8 => ExStat::Intelligence,
                        value => anyhow::bail!("unknown EX stat modifier {value}"),
                    };
                    Ok(ExStatBonus {
                        stat,
                        percent: r[1],
                    })
                })
                .collect::<Result<_>>()?;
            let tendency = match half(row, 16)? as i16 {
                -1 => Some(ExTendency::Technical),
                0 => None,
                1 => Some(ExTendency::Strike),
                value => anyhow::bail!("unknown EX skill tendency {value}"),
            };
            let activation = match row[19] {
                0 => ExActivation::Constant,
                1 => ExActivation::Chance,
                2 => ExActivation::BattleEnd,
                3 => ExActivation::Other,
                value => anyhow::bail!("unknown EX skill activation {value}"),
            };
            Ok((
                id,
                ExSkill {
                    name: dol::text(executable, word(row, 4)?)?,
                    description: super::text::paragraph(executable, word(row, 8)?)?.0,
                    stat_bonuses,
                    save_point_tp_cost: (u16::from(id) == save_point_skill)
                        .then_some(save_point_cost),
                    tendency,
                    activation,
                },
            ))
        })
        .collect::<Result<_>>()?;
    let text = |offset| {
        dol::text(
            executable,
            word(dol::slice(executable, LABELS + offset, 4)?, 0)?,
        )
    };
    Ok(ExSkillData {
        skills,
        characters,
        gem_items: [40, 41, 42, 43, 496],
        activation_labels: [
            ExActivation::Constant,
            ExActivation::Chance,
            ExActivation::BattleEnd,
            ExActivation::Other,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| Ok((kind, text(44 + i as u32 * 4)?)))
        .collect::<Result<_>>()?,
        labels: [
            ("title", 0),
            ("set_gem", 60),
            ("replace_gem", 64),
            ("yes", 68),
            ("no", 72),
            ("hp", 4),
            ("tp", 8),
            ("slash", 12),
            ("thrust", 16),
            ("defense", 20),
            ("accuracy", 24),
            ("evasion", 28),
            ("intelligence", 32),
            ("luck", 36),
            ("attack", 40),
        ]
        .into_iter()
        .map(|(key, offset)| Ok((key.into(), text(offset)?)))
        .chain(
            [
                ("gem_max", 0x8035d658),
                ("gem_level", 0x8035d65c),
                ("gem_empty", 0x8035d664),
            ]
            .into_iter()
            .map(|(key, address)| Ok((key.into(), dol::text(executable, address)?))),
        )
        .collect::<Result<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::menu_data::MenuSpan;

    #[test]
    #[ignore = "requires the locally extracted GQSEAF executable"]
    fn ex_catalog_resolves_character_recipes_and_typed_effects() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol");
        let executable = fs::read(path).unwrap();
        let data = cook(&executable).unwrap();
        data.validate(528).unwrap();
        assert_eq!(data.skills.len(), 136);
        assert_eq!(
            data.characters
                .iter()
                .map(|c| c.compounds.len())
                .sum::<usize>(),
            216
        );
        let strong = &data.skills[&1];
        assert_eq!(strong.name, "Strong");
        assert_eq!(strong.tendency, Some(ExTendency::Strike));
        assert_eq!(strong.stat_bonuses[0].stat, ExStat::Strength);
        assert_eq!(strong.stat_bonuses[0].percent, 5);
        assert!(
            data.skills[&3]
                .description
                .lines
                .iter()
                .flatten()
                .any(|s| matches!(s, MenuSpan::Button { sprite: 23 }))
        );
        assert_eq!(data.characters[0].compounds[0].required, [1, 2]);
        assert_eq!(data.characters[0].compounds[0].skill, 54);
        assert_eq!(data.skills[&54].name, "EX Attack");
        assert_eq!(data.characters[2].levels[0], [23, 2, 3, 5]);
        assert_eq!(data.skills[&24].name, "Personal");
        assert_eq!(data.skills[&24].save_point_tp_cost, None);
        assert_eq!(data.skills[&31].save_point_tp_cost, Some(1));
        assert!(
            !data.skills.contains_key(&17),
            "unused placeholder was deployed"
        );

        let mut json = serde_json::to_value(&data).unwrap();
        json["skills"]["1"]["stat_bonuses"][0]["percent"] = 7.into();
        let mut edited: ExSkillData = serde_json::from_value(json).unwrap();
        edited.validate(528).unwrap();
        assert_eq!(edited.skills[&1].stat_bonuses[0].percent, 7);
        edited.characters[0].compounds[0].required[0] = 255;
        assert!(
            edited.validate(528).is_err(),
            "dangling recipe reference was accepted"
        );
        assert!(
            cook(&executable[..256]).is_err(),
            "truncated executable was accepted"
        );
    }
}
