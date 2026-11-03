//! Pure catalogue expansion for the visual capture harness.
use anyhow::{Context, Result, ensure};
use resonance_content::{battle_unison::Catalogue, menu_data::MenuData, session::SessionData};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Assignment {
    pub recipe: usize,
    pub members: Vec<(u8, u16)>,
}

pub(super) fn enumerate(
    catalogue: &Catalogue,
    session: &SessionData,
    menus: &MenuData,
    combination: u8,
    selected_recipe: Option<usize>,
) -> Result<Vec<Assignment>> {
    let row = catalogue
        .combinations
        .iter()
        .find(|row| row.id == combination)
        .with_context(|| format!("unknown Unison combination {combination}"))?;
    ensure!(
        !row.recipes.is_empty(),
        "Unison combination {combination} has no source recipes"
    );
    ensure!(
        selected_recipe.is_none_or(|index| index < row.recipes.len()),
        "Unison recipe index exceeds combination {combination}"
    );
    fn expand(
        choices: &[Vec<(u8, u16)>],
        members: &mut Vec<(u8, u16)>,
        completed: &mut BTreeSet<Vec<(u8, u16)>>,
    ) {
        if members.len() == choices.len() {
            completed.insert(members.clone());
            return;
        }
        for &candidate in &choices[members.len()] {
            if members.iter().any(|member| member.0 == candidate.0) {
                continue;
            }
            members.push(candidate);
            expand(choices, members, completed);
            members.pop();
        }
    }
    let mut assignments = Vec::new();
    for (recipe, required) in row
        .recipes
        .iter()
        .enumerate()
        .filter(|(index, _)| selected_recipe.is_none_or(|selected| selected == *index))
    {
        ensure!(
            (2..=4).contains(&required.len()) && required.len() == usize::from(row.participants),
            "invalid Unison combination {combination} recipe {recipe}"
        );
        let mut choices = Vec::new();
        for &native in required {
            let mut owners = BTreeSet::new();
            for (index, character) in session.characters.iter().enumerate() {
                let owner =
                    u8::try_from(index + 1).context("party character ID exceeds capture format")?;
                for &arte in &character.allowed_techniques {
                    if catalogue
                        .placements
                        .get(&arte)
                        .is_some_and(|placement| placement.native_id == native)
                    {
                        let technique =
                            menus.techniques.get(usize::from(arte)).with_context(|| {
                                format!(
                                    "missing Unison eligibility for character {owner} arte {arte}"
                                )
                            })?;
                        if technique.unison_usable {
                            owners.insert((owner, arte));
                        }
                    }
                }
            }
            choices.push(owners.into_iter().collect());
        }
        let mut completed = BTreeSet::new();
        expand(&choices, &mut Vec::new(), &mut completed);
        ensure!(
            !completed.is_empty(),
            "no legal distinct party owners for Unison combination {combination} recipe {recipe}"
        );
        assignments.extend(
            completed
                .into_iter()
                .map(|members| Assignment { recipe, members }),
        );
    }
    ensure!(!assignments.is_empty(), "no Unison assignments selected");
    Ok(assignments)
}
