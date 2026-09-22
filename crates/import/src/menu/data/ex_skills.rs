//! Resolve skill definitions and character-specific compound recipes into JSON.
use super::*;
use crate::all_assets::ex_skills::{Catalogue, Label};
use resonance_content::menu_data::{
    CharacterExSkills, CompoundExSkill, ExActivation, ExSkill, ExSkillData, ExStat, ExStatBonus,
    ExTendency,
};
use std::collections::BTreeSet;

pub(super) fn cook(catalogue: &Catalogue) -> Result<ExSkillData> {
    let caster = usize::try_from(catalogue.save_point_rule.character_index)?;
    let save_point_skill = *catalogue
        .personal_skills
        .get(caster)
        .context("invalid save-point TP caster")?;
    let save_point_cost: u8 = catalogue.save_point_rule.tp_cost.try_into()?;
    let characters: Vec<_> = catalogue
        .characters
        .iter()
        .map(|row| {
            ensure!(row.compound_count == 24, "invalid compound EX skill count");
            Ok(CharacterExSkills {
                levels: row.levels,
                compounds: row
                    .compounds
                    .iter()
                    .map(|r| {
                        let count = usize::from(r.requirement_count);
                        ensure!(
                            (2..=4).contains(&count),
                            "invalid compound EX skill requirements"
                        );
                        Ok(CompoundExSkill {
                            skill: r.skill.try_into()?,
                            required: r.requirements[..count].to_vec(),
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
    let skills = referenced
        .into_iter()
        .map(|id| {
            let row = catalogue
                .definitions
                .get(usize::from(id))
                .context("EX skill reference outside definitions")?;
            ensure!(
                row.id == u32::from(id),
                "unsupported EX skill definition {id}"
            );
            Ok((
                id,
                ExSkill {
                    name: catalogue.required_text(row.name)?.to_owned(),
                    description: text::decode(catalogue.required_text(row.description)?, 9)?,
                    stat_bonuses: row
                        .stat_bonuses
                        .iter()
                        .filter(|bonus| bonus.selector != 0 || bonus.percent != 0)
                        .map(|bonus| {
                            Ok(ExStatBonus {
                                stat: match bonus.selector {
                                    1 => ExStat::Strength,
                                    2 => ExStat::Defense,
                                    3 => ExStat::Accuracy,
                                    4 => ExStat::Evasion,
                                    5 => ExStat::MaxHp,
                                    6 => ExStat::MaxTp,
                                    7 => ExStat::Luck,
                                    8 => ExStat::Intelligence,
                                    other => anyhow::bail!(
                                        "unsupported EX stat selector {other} in skill {id}"
                                    ),
                                },
                                percent: bonus.percent,
                            })
                        })
                        .collect::<Result<_>>()?,
                    save_point_tp_cost: (u16::from(id) == save_point_skill)
                        .then_some(save_point_cost),
                    tendency: match row.tendency {
                        -1 => Some(ExTendency::Technical),
                        0 => None,
                        1 => Some(ExTendency::Strike),
                        other => anyhow::bail!("unsupported EX tendency {other} in skill {id}"),
                    },
                    activation: match row.activation {
                        0 => ExActivation::Constant,
                        1 => ExActivation::Chance,
                        2 => ExActivation::BattleEnd,
                        3 => ExActivation::Other,
                        other => anyhow::bail!("unsupported EX activation {other} in skill {id}"),
                    },
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok(ExSkillData {
        skills,
        characters,
        gem_items: [40, 41, 42, 43, 496],
        activation_labels: [
            (ExActivation::Constant, Label::Constant),
            (ExActivation::Chance, Label::Chance),
            (ExActivation::BattleEnd, Label::BattleEnd),
            (ExActivation::Other, Label::Other),
        ]
        .into_iter()
        .map(|(kind, label)| Ok((kind, catalogue.label(label)?.to_owned())))
        .collect::<Result<_>>()?,
        labels: [
            ("title", Label::Title),
            ("set_gem", Label::SetGem),
            ("replace_gem", Label::ReplaceGem),
            ("yes", Label::Yes),
            ("no", Label::No),
            ("hp", Label::Hp),
            ("tp", Label::Tp),
            ("slash", Label::Slash),
            ("thrust", Label::Thrust),
            ("defense", Label::Defense),
            ("accuracy", Label::Accuracy),
            ("evasion", Label::Evasion),
            ("intelligence", Label::Intelligence),
            ("luck", Label::Luck),
            ("attack", Label::Attack),
        ]
        .into_iter()
        .map(|(key, label)| Ok((key.into(), catalogue.label(label)?.to_owned())))
        .chain(
            [
                ("gem_max", catalogue.formats.gem_max),
                ("gem_level", catalogue.formats.gem_level),
                ("gem_empty", catalogue.formats.gem_empty),
            ]
            .into_iter()
            .map(|(key, reference)| Ok((key.into(), catalogue.text(reference).to_owned()))),
        )
        .collect::<Result<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::menu_data::{ExStat, ExTendency, MenuSpan};

    #[test]
    #[ignore = "requires the locally extracted GQSEAF executable"]
    fn ex_catalog_resolves_character_recipes_and_typed_effects() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol");
        let executable = fs::read(path).unwrap();
        let mut catalogue = crate::all_assets::ex_skills::read(&executable).unwrap();
        let data = cook(&catalogue).unwrap();
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
            crate::all_assets::ex_skills::read(&executable[..256]).is_err(),
            "truncated executable was accepted"
        );
        // Inactive definitions and unused requirement slots do not constrain runtime admission.
        catalogue.definitions[17].activation = 255;
        catalogue.definitions[17].tendency = i16::MIN;
        catalogue.definitions[17].stat_bonuses[0].selector = 255;
        catalogue.characters[0].compounds[0].requirements[3] = 255;
        assert_eq!(
            serde_json::to_value(cook(&catalogue).unwrap()).unwrap(),
            serde_json::to_value(&data).unwrap()
        );
        catalogue.definitions[1].activation = 255;
        assert!(
            cook(&catalogue).is_err(),
            "unsupported selected activation was admitted"
        );
        catalogue.definitions[1].activation = 0;
        catalogue.definitions[1].tendency = i16::MIN;
        assert!(
            cook(&catalogue).is_err(),
            "unsupported selected tendency was admitted"
        );
        catalogue.definitions[1].tendency = 1;
        catalogue.definitions[1].stat_bonuses[0].selector = 255;
        assert!(
            cook(&catalogue).is_err(),
            "unsupported selected stat was admitted"
        );
    }
}
