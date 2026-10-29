use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initial {
    pub stats: BTreeMap<Statistic, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub equipment: BTreeMap<Slot, String>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub level: u8,
    #[serde(default = "luck", skip_serializing_if = "default_luck")]
    pub luck: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub experience: u32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub affinity: i32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub overlimit: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub technique_balance: i8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub techniques: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shortcuts: BTreeMap<Direction, String>,
    /// Named recipes from game/cooking.json; only nonzero mastery is stored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cooking: BTreeMap<String, u8>,
    /// Physical gem slots 1 through 4.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ex_gems: BTreeMap<u8, Gem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_ex_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_compound_ex_skills: Vec<String>,
}
fn one() -> u8 {
    1
}
fn is_one(v: &u8) -> bool {
    *v == 1
}
fn luck() -> u8 {
    50
}
fn default_luck(v: &u8) -> bool {
    *v == 50
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Neutral,
    Up,
    Down,
    Side,
}
impl Direction {
    const ALL: [Self; 4] = [Self::Neutral, Self::Up, Self::Down, Self::Side];
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gem {
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
}

impl Initial {
    pub(super) fn from_parts(
        source: &CharacterDefinition,
        items: &Mappings,
        techniques: &Mappings,
        recipes: &Mappings,
        ex: &crate::ex_skills::Document,
        character: crate::overworld::PartyCharacter,
    ) -> Result<Self> {
        Ok(Self {
            stats: Statistic::ALL
                .into_iter()
                .zip(source.base_stats)
                .filter(|(_, n)| *n != 0)
                .map(|(stat, n)| (stat, f64::from(n) / stat.scale()))
                .collect(),
            equipment: Slot::ALL
                .into_iter()
                .zip(source.equipment)
                .filter(|(_, id)| *id != 0)
                .map(|(slot, id)| Ok((slot, items.target(u32::from(id))?.into())))
                .collect::<Result<_>>()?,
            level: source.level,
            luck: source.luck,
            experience: source.experience,
            affinity: source.affinity,
            overlimit: source.overlimit,
            technique_balance: source.technique_balance,
            techniques: source
                .techniques
                .iter()
                .map(|&id| Ok(techniques.target(u32::from(id))?.into()))
                .collect::<Result<_>>()?,
            shortcuts: Direction::ALL
                .into_iter()
                .zip(source.shortcuts)
                .filter(|(_, id)| *id != 0)
                .map(|(slot, id)| Ok((slot, techniques.target(u32::from(id))?.into())))
                .collect::<Result<_>>()?,
            cooking: source
                .cooking
                .iter()
                .enumerate()
                .filter(|(_, n)| **n != 0)
                .map(|(id, &n)| Ok((recipes.target(id as u32)?.into(), n)))
                .collect::<Result<_>>()?,
            ex_gems: source
                .ex_gems
                .iter()
                .zip(source.ex_skills)
                .enumerate()
                .filter(|(_, (level, skill))| **level != 0 || *skill != 0)
                .map(|(slot, (&level, skill))| {
                    Ok((
                        slot as u8 + 1,
                        Gem {
                            level,
                            skill: (skill != 0)
                                .then(|| ex.mappings.target(u32::from(skill)).map(str::to_owned))
                                .transpose()?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            compound_ex_skills: source
                .compound_ex_skills
                .iter()
                .map(|&id| Ok(ex.compounds(character)?.target(u32::from(id))?.into()))
                .collect::<Result<_>>()?,
            recent_compound_ex_skills: source
                .recent_compound_ex_skills
                .iter()
                .map(|&id| Ok(ex.compounds(character)?.target(u32::from(id))?.into()))
                .collect::<Result<_>>()?,
        })
    }
    pub(super) fn prepare(
        &self,
        items: &BTreeMap<&str, u32>,
        techniques: &BTreeMap<&str, u32>,
        recipes: &BTreeMap<&str, u32>,
        ex: &crate::ex_skills::Document,
        character: crate::overworld::PartyCharacter,
    ) -> Result<CharacterDefinition> {
        let reference = |map: &BTreeMap<&str, u32>, name: &str| -> Result<u16> {
            Ok(u16::try_from(*map.get(name).with_context(|| {
                format!("unknown initial resource {name}")
            })?)?)
        };
        let mut stats = [0; 7];
        for (i, stat) in Statistic::ALL.into_iter().enumerate() {
            stats[i] = units(
                stat,
                self.stats.get(&stat).copied().unwrap_or_default(),
                32767,
            )?;
        }
        let mut equipment = [0; 6];
        for (&slot, name) in &self.equipment {
            equipment[slot as usize] = reference(items, name)?;
        }
        let mut shortcuts = [0; 4];
        for (&slot, name) in &self.shortcuts {
            shortcuts[slot as usize] = reference(techniques, name)?;
        }
        let mut cooking = [0; menu_data::RECIPE_COUNT];
        for (recipe, &mastery) in &self.cooking {
            ensure!(mastery <= 8, "initial cooking mastery must be 0 through 8");
            *cooking
                .get_mut(usize::from(reference(recipes, recipe)?))
                .context("invalid initial recipe")? = mastery;
        }
        let (mut ex_gems, mut ex_skills) = ([0; 4], [0; 4]);
        for (&slot, gem) in &self.ex_gems {
            ensure!(
                (1..=4).contains(&slot),
                "EX gem slots are numbered 1 through 4"
            );
            ex_gems[usize::from(slot - 1)] = gem.level;
            ex_skills[usize::from(slot - 1)] = gem
                .skill
                .as_ref()
                .map(|name| ex.skill_id(name))
                .transpose()?
                .unwrap_or_default();
        }
        Ok(CharacterDefinition {
            base_stats: stats,
            equipment,
            shortcuts,
            cooking,
            ex_gems,
            ex_skills,
            level: self.level,
            luck: self.luck,
            experience: self.experience,
            affinity: self.affinity,
            overlimit: self.overlimit,
            technique_balance: self.technique_balance,
            techniques: self
                .techniques
                .iter()
                .map(|name| reference(techniques, name))
                .collect::<Result<_>>()?,
            compound_ex_skills: self
                .compound_ex_skills
                .iter()
                .map(|name| ex.compound_id(character, name))
                .collect::<Result<_>>()?,
            recent_compound_ex_skills: self
                .recent_compound_ex_skills
                .iter()
                .map(|name| ex.compound_id(character, name))
                .collect::<Result<_>>()?,
            allowed_techniques: vec![],
            level_techniques: BTreeMap::new(),
            growth: std::array::from_fn(|_| StatGrowth {
                base: 0,
                random: 0,
                title_bonus: 0,
            }),
        })
    }
}
