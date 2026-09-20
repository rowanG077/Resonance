//! Bind selected action owners and prepare their native controller policies.
use super::*;

pub(crate) struct Cooker<'a> {
    extracted: &'a Path,
    catalogue: &'a crate::arte::Catalogue,
    pub(super) rel: Rel,
    native_costs: BTreeMap<u16, u16>,
}

#[test]
#[ignore = "requires both extracted discs, cook-all and the accepted prepared selection; metadata only"]
fn original_cooked_enemy_actions_match_prepared_selection_on_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let output = local.join("all-assets");
    let ids = [2, 3, 8, 9, 10, 11, 36];
    let expected: Vec<EnemyActions> =
        if let Some(path) = std::env::var_os("RESONANCE_ENEMY_ACTIONS_BASELINE") {
            serde_json::from_slice::<Vec<EnemyActions>>(&fs::read(path)?)?
        } else {
            serde_json::from_value::<Vec<EnemyActions>>(
                serde_json::from_slice::<serde_json::Value>(&fs::read(
                    output.join("battle/catalog.json"),
                )?)?["actions"]["enemies"]
                    .take(),
            )?
        }
        .into_iter()
        .filter(|enemy: &EnemyActions| ids.contains(&enemy.monster))
        .collect();
    ensure!(
        expected
            .iter()
            .map(|enemy| enemy.monster)
            .collect::<Vec<_>>()
            == ids,
        "incomplete accepted enemy fixture"
    );
    let expected: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&expected)?)?;
    let catalogue = crate::arte::cooked(&output.join("data"))?;
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let sources = crate::battle::all::Sources::cooked(&output, disc)?;
        let source = crate::cooked::Source::open(&output, disc, &sources.enemy)?;
        let lengths = enemy::VoiceDurations::bind(&crate::cooked::Source::open(
            &output,
            disc,
            &sources.usual,
        )?)?;
        let cooker = Cooker::read(&extracted, &catalogue)?;
        let tables = Tables::bind(&output, disc, &sources)?;
        assert_eq!(
            lengths,
            enemy::VoiceDurations::original(&fs::read(
                extracted.join("files").join(&sources.usual)
            )?)?
        );
        let actual = ids
            .into_iter()
            .map(|id| cooker.enemy_actions(&source, &tables, &lengths, id))
            .collect::<Result<Vec<_>>>()?;
        let actual: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&actual)?)?;
        ensure!(
            actual == expected,
            "disc {disc} cooked enemy actions differ from accepted selection"
        );
    }
    Ok(())
}

impl<'a> Cooker<'a> {
    pub fn read(extracted: &'a Path, catalogue: &'a crate::arte::Catalogue) -> Result<Self> {
        Ok(Self {
            extracted,
            catalogue,
            rel: Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?,
            native_costs: enemy::native_costs(catalogue),
        })
    }

    pub(super) fn selected(
        &mut self,
        data: &Path,
        motion: &super::super::motion::MotionTables,
        selection: &super::super::CookSelection,
        monsters: &[u8],
        shared_contact: &mut impl FnMut() -> Result<ProjectileRecipe>,
    ) -> Result<BattleActions> {
        let output = data
            .parent()
            .context("battle data has no cooked library root")?;
        let disc = crate::disc_number(self.extracted)?;
        let sources = crate::battle::all::Sources::cooked(output, disc)?;
        let source = crate::cooked::Source::open(output, disc, &sources.enemy)?;
        let mut tables = Tables::bind(output, disc, &sources)?;
        let lengths = enemy::VoiceDurations::bind(&crate::cooked::Source::open(
            output,
            disc,
            &sources.usual,
        )?)?;
        let mut result = BattleActions {
            party: normals::bind(data, &selection.party)?,
            enemies: monsters
                .iter()
                .map(|&id| self.enemy_actions(&source, &tables, &lengths, id))
                .collect::<Result<_>>()?,
            techniques: Vec::new(),
            chains: None,
            projectiles: motion.bind_projectiles()?,
        };
        let selected = required_techniques(self.catalogue, &selection.artes, &result.enemies)?;
        tables.select(output, disc, &sources, self.catalogue, &selected)?;
        result.techniques = prepare_technique_actions(
            self.catalogue,
            &self.rel,
            &tables,
            &selected,
            shared_contact,
        )?;
        result.chains = Some(martial_chains(
            self.catalogue,
            &tables.martial.chains,
            &selected,
        )?);
        result.validate()?;
        Ok(result)
    }

    fn enemy_actions(
        &self,
        source: &crate::cooked::Source<'_>,
        tables: &Tables,
        lengths: &enemy::VoiceDurations,
        monster: u8,
    ) -> Result<EnemyActions> {
        let records = binding::Records::bind(source, monster)?;
        let mut definition = prepared_enemy_actions(&records, monster)
            .with_context(|| format!("enemy {monster} actions"))?;
        definition.policy = Some(records.policy(source, monster, &tables.enemy, lengths)?);
        definition.casting = enemy::prepared_casting(
            &records,
            self.catalogue,
            lengths,
            tables.recovery.stored_release_rate,
        )?;
        for action in &definition.actions {
            if let Some(native) = action.technique {
                definition.native_tp.insert(
                    native,
                    *self
                        .native_costs
                        .get(&native)
                        .context("enemy native arte has no TP record")?,
                );
            }
        }
        Ok(definition)
    }
}
