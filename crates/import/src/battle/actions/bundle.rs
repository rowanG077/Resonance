//! Bind selected actions and actor settings to their shared cooked records.
use super::*;
#[cfg(test)]
use crate::battle::embedded::{Layout, SETTINGS_BYTES};
use crate::{
    battle::{
        action_program::PHASE_COUNT,
        all::{ActorSettings, PartySettings},
        animation_table,
        embedded::PARTY_COUNT,
    },
    cooked::Source,
};
use serde::Deserialize;

pub(super) struct Tables {
    bundles: BTreeMap<u16, Bundle>,
    actors: Vec<PartySettings>,
    pub voices: crate::battle::casting_voices::CastingTables,
    pub programs: crate::battle::casting_programs::Programs,
    pub ordinary: ordinary_parameters::Parameters,
    pub stored: stored_parameters::Parameters,
    pub elemental: elemental_parameters::Parameters,
    pub recovery: recovery_parameters::Parameters,
    pub summons: summon_parameters::Parameters,
    pub martial: martial::Parameters,
    pub enemy: enemy::Parameters,
}

impl Tables {
    pub fn bind(output: &Path, disc: u8, sources: &crate::battle::all::Sources) -> Result<Self> {
        let usual = Source::open(output, disc, &sources.usual)?;
        let module = Source::open(output, disc, "US_r_Top2Btl.rel")?;
        Ok(Self {
            actors: module.embedded("battle-party-settings", "US_r_Top2Btl.rel")?,
            bundles: BTreeMap::new(),
            voices: crate::battle::casting_voices::CastingTables::bind(&module, &usual)?,
            programs: crate::battle::casting_programs::Programs::bind(&module)?,
            ordinary: module.embedded("battle-ordinary-spell-parameters", "US_r_Top2Btl.rel")?,
            stored: module.embedded("battle-stored-spell-parameters", "US_r_Top2Btl.rel")?,
            elemental: module.embedded("battle-elemental-spell-parameters", "US_r_Top2Btl.rel")?,
            recovery: module.embedded("battle-recovery-parameters", "US_r_Top2Btl.rel")?,
            summons: module.embedded("battle-summon-parameters", "US_r_Top2Btl.rel")?,
            martial: module.embedded("battle-martial-parameters", "US_r_Top2Btl.rel")?,
            enemy: module.embedded("battle-enemy-parameters", "US_r_Top2Btl.rel")?,
        })
    }

    pub fn select(
        &mut self,
        output: &Path,
        disc: u8,
        sources: &crate::battle::all::Sources,
        catalogue: &crate::arte::Catalogue,
        techniques: &[u16],
    ) -> Result<()> {
        let usual = Source::open(output, disc, &sources.usual)?;
        let magic = Source::open(
            output,
            disc,
            sources.archive(crate::battle::all::Archive::Magic),
        )?;
        self.bundles = action_bundle_ids(catalogue, techniques)?
            .into_iter()
            .map(|native| {
                let (source, path) = match Origin::for_native(catalogue, native)? {
                    Origin::Common { table, index } => {
                        (&usual, format!("battle/usual/{table}/{index}/actions.json"))
                    }
                    Origin::Stored { package } => (
                        &magic,
                        format!("battle/magic-{package}/member-100/actions.json"),
                    ),
                };
                Ok((native, Bundle::bind(source, &path)?))
            })
            .collect::<Result<_>>()?;
        Ok(())
    }

    pub fn bundle(&self, native: u16) -> Result<&Bundle> {
        self.bundles
            .get(&native)
            .with_context(|| format!("missing cooked action bundle {native}"))
    }

    pub fn actor(&self, character: u8) -> Result<&ActorSettings> {
        ensure!(
            (1..=PARTY_COUNT).contains(&character),
            "invalid party settings owner"
        );
        self.actors
            .iter()
            .find(|row| row.character == character)
            .map(|row| &row.settings)
            .context("missing cooked party settings")
    }

    #[cfg(test)]
    pub fn original(extracted: &Path, rel: &Rel, usual: &[u8], natives: &[u16]) -> Result<Self> {
        let catalogue = crate::arte::read(&fs::read(extracted.join("sys/main.dol"))?)?;
        let mut magic = None;
        let mut bundles = BTreeMap::new();
        for &native in natives {
            let bytes = match Origin::for_native(&catalogue, native)? {
                Origin::Common { table, index } => {
                    member(member(usual, table)?, usize::from(index))?
                }
                Origin::Stored { package } => {
                    if magic.is_none() {
                        magic = Some(crate::battle::effect_program::MagicArchive::read(
                            extracted,
                        )?);
                    }
                    crate::battle::effect_program::magic_member(
                        magic.as_ref().unwrap().package(package)?,
                        256,
                    )?
                    .context("stored spell has no action bundle")?
                }
            };
            bundles.insert(native, Bundle::decode(bytes)?);
        }
        Ok(Self {
            bundles,
            actors: (1..=PARTY_COUNT)
                .map(|character| {
                    Ok(PartySettings {
                        character,
                        settings: ActorSettings::read(rel.at((
                            DATA,
                            Layout::RETAIL.party_settings
                                + usize::from(character - 1) * SETTINGS_BYTES,
                        ))?)?,
                    })
                })
                .collect::<Result<_>>()?,
            voices: crate::battle::casting_voices::CastingTables::original(rel, usual)?,
            programs: crate::battle::casting_programs::Programs::original(rel)?,
            ordinary: ordinary_parameters::Parameters::read(rel)?,
            stored: stored_parameters::Parameters::read(rel)?,
            elemental: elemental_parameters::Parameters::read(rel)?,
            recovery: recovery_parameters::Parameters::read(rel)?,
            summons: summon_parameters::Parameters::read(rel)?,
            martial: martial::Parameters::read(rel)?,
            enemy: enemy::Parameters::read(rel)?,
        })
    }
}

enum Origin {
    Common { table: usize, index: u16 },
    Stored { package: u16 },
}

impl Origin {
    fn for_native(catalogue: &crate::arte::Catalogue, native: u16) -> Result<Self> {
        if native < 200 {
            return Ok(Self::Common {
                table: 8,
                index: native,
            });
        }
        let definition = catalogue
            .definitions
            .iter()
            .find(|row| row.native_id as u16 == native)
            .context("action native has no arte definition")?;
        let index = native - 200;
        // Stored releases replace the common phase table with package member 256.
        Ok(if definition.flags & 0x1000_0001 != 0 {
            Self::Stored { package: index }
        } else {
            Self::Common { table: 9, index }
        })
    }
}

#[derive(Deserialize)]
pub(in crate::battle) struct Phase {
    pub duration: u16,
    pub recovery_ticks: u16,
    pub buffer_until: u16,
    pub combo_at: u16,
    pub startup_effect: Option<u16>,
    pub hit_rule_root: usize,
    pub hit_root: Option<usize>,
    pub animation_root: Option<usize>,
    commands: Commands,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::action_program::{PhaseRecord, PhaseTable};

    #[test]
    #[ignore = "requires original discs and their cooked publications"]
    fn original_cooked_action_bundles_preserve_phase_programs_and_rules() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let usual = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
            let magic = crate::battle::effect_program::MagicArchive::read(&extracted)?;
            let output = local.join("all-assets");
            let catalogue = crate::arte::cooked(&output.join("data"))?;
            let natives = [
                4, 6, 10, 11, 14, 20, 22, 34, 37, 39, 40, 41, 42, 43, 44, 63, 64, 65, 66, 67, 68,
                69, 70, 71, 85, 87, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211,
                212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 226, 227, 228,
                229, 230, 231, 232, 233, 236, 251, 252, 253, 278, 283, 284, 285, 286, 287, 288,
                289, 290, 291, 292, 293,
            ];
            let techniques = natives
                .iter()
                .map(|&native| {
                    catalogue
                        .definitions
                        .iter()
                        .position(|row| row.native_id == native)
                        .map(|index| index as u16)
                        .context("missing fixture technique")
                })
                .collect::<Result<Vec<_>>>()?;
            let sources = crate::battle::all::Sources::cooked(&output, disc)?;
            let mut tables = Tables::bind(&output, disc, &sources)?;
            tables.select(&output, disc, &sources, &catalogue, &techniques)?;
            let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
            assert_eq!(
                serde_json::to_vec(&tables.enemy)?,
                serde_json::to_vec(&enemy::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.martial)?,
                serde_json::to_vec(&martial::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.ordinary)?,
                serde_json::to_vec(&ordinary_parameters::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.stored)?,
                serde_json::to_vec(&stored_parameters::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.recovery)?,
                serde_json::to_vec(&recovery_parameters::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.elemental)?,
                serde_json::to_vec(&elemental_parameters::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.summons)?,
                serde_json::to_vec(&summon_parameters::Parameters::read(&rel)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.programs.animation()?)?,
                serde_json::to_vec(&animations_at(rel.at((DATA, 0))?, 0x11e0)?)?
            );
            assert_eq!(
                serde_json::to_vec(&tables.programs.commands()?)?,
                serde_json::to_vec(&commands(rel.at((DATA, 0x1210))?)?)?
            );
            for character in 1..=PARTY_COUNT {
                let casting = &tables.actor(character)?.casting;
                let bytes = rel.at((
                    DATA,
                    Layout::RETAIL.party_settings + usize::from(character - 1) * SETTINGS_BYTES,
                ))?;
                assert_eq!(
                    (
                        casting.loop_start,
                        casting.animation_rate.to_bits(),
                        casting.resume_start,
                        casting.resume_blend_ticks,
                        casting.resume_loop_start,
                        casting.stored_recovery_clip
                    ),
                    (
                        bytes[0x9d],
                        word(bytes, 0xa0)?,
                        bytes[0xa4],
                        bytes[0xa5],
                        bytes[0xa8],
                        bytes[0xa6]
                    ),
                );
                assert_eq!(
                    (
                        casting.chant_looping,
                        casting.release_looping,
                        casting.stored_release_looping
                    ),
                    (
                        bytes[0x30] & 4 == 0,
                        bytes[0x30] & 2 != 0,
                        bytes[0x30] & 1 != 0
                    )
                );
            }
            for native in natives.map(|id| id as u16) {
                let bundle = tables.bundle(native)?;
                let bytes = match Origin::for_native(&catalogue, native)? {
                    Origin::Common { table, index } => {
                        member(member(&usual, table)?, usize::from(index))?
                    }
                    Origin::Stored { package } => {
                        crate::battle::effect_program::magic_member(magic.package(package)?, 256)?
                            .context("missing original action bundle")?
                    }
                };
                let rule_start = word(bytes, 0)? as usize;
                assert_eq!(
                    bundle.rule_count(),
                    (word(bytes, 4)? as usize - rule_start) / RULE_BYTES
                );
                for index in 0..bundle.rule_count() {
                    assert_eq!(
                        serde_json::to_vec(&bundle.rule(index)?)?,
                        serde_json::to_vec(&hit_rule(&bytes[rule_start + index * RULE_BYTES..])?)?
                    );
                }
                for (variant, phase) in bundle.phases.iter().enumerate() {
                    let original = PhaseRecord::read(bytes, variant)?;
                    assert_eq!(
                        (
                            phase.duration,
                            phase.recovery_ticks,
                            phase.buffer_until,
                            phase.combo_at,
                            phase.startup_effect
                        ),
                        (
                            original.duration,
                            original.recovery_ticks,
                            original.buffer_until,
                            original.combo_at,
                            original.effect()?
                        ),
                    );
                    if native >= 200 || phase.duration == 0 {
                        continue;
                    }
                    let (commands, loop_commands) =
                        commands(&bytes[original.table_start(bytes, PhaseTable::Commands)?..])?;
                    let expected = Action {
                        duration: original.duration,
                        tp: 7,
                        animations: animations_at(
                            bytes,
                            original.table_start(bytes, PhaseTable::Animations)?,
                        )?,
                        commands,
                        loop_commands,
                        hits: hits(
                            &bytes[original.table_start(bytes, PhaseTable::Hits)?..],
                            &bytes[original.table_start(bytes, PhaseTable::HitRules)?..],
                        )?,
                    };
                    assert_eq!(
                        serde_json::to_vec(&bundle.action(variant, 7)?)?,
                        serde_json::to_vec(&expected)?,
                        "disc{disc} action {native} phase {variant}"
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Commands {
    commands: Vec<crate::battle::action_program::Command>,
    loops: bool,
    initialized: Option<bool>,
}

#[derive(Deserialize)]
struct Rules {
    records: Vec<HitRuleRecord>,
}

#[derive(Deserialize)]
struct Hits {
    records: Vec<HitEntry>,
}

#[derive(Deserialize)]
struct HitEntry {
    offset: usize,
    record: HitRecord,
}

#[derive(Deserialize)]
pub(in crate::battle) struct Bundle {
    pub phases: Vec<Phase>,
    animations: animation_table::Parsed,
    hit_rules: Rules,
    hit_records: Hits,
}

impl Bundle {
    pub fn bind(source: &Source<'_>, path: &str) -> Result<Self> {
        let (_, bytes) = source.resolve(path)?;
        let bundle: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            bundle.phases.len() == PHASE_COUNT,
            "expected four action phases"
        );
        Ok(bundle)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_value(
            crate::battle::action_program::physical_bundle(bytes)?,
        )?)
    }

    pub fn rule_count(&self) -> usize {
        self.hit_rules.records.len()
    }

    pub fn rule(&self, index: usize) -> Result<HitRule> {
        self.hit_rules
            .records
            .get(index)
            .context("missing action hit rule")?
            .lower()
    }

    pub fn phase_rule(&self, variant: usize, index: usize) -> Result<HitRule> {
        let root = self.phase(variant)?.hit_rule_root;
        ensure!(
            root.is_multiple_of(RULE_BYTES),
            "unaligned action rule root"
        );
        self.rule(root / RULE_BYTES + index)
    }

    pub fn phase(&self, variant: usize) -> Result<&Phase> {
        self.phases.get(variant).context("missing action phase")
    }

    pub fn hit_record(&self, offset: usize) -> Result<&HitRecord> {
        self.hit_records
            .records
            .iter()
            .find(|entry| entry.offset == offset)
            .map(|entry| &entry.record)
            .context("missing cooked action hit record")
    }

    pub fn hits_at(&self, variant: usize, root: usize) -> Result<Vec<HitWindow>> {
        let mut hits = Vec::new();
        for offset in (root..).step_by(HIT_BYTES).take(LIMIT) {
            match self
                .hit_record(offset)?
                .lower_with(|index| self.phase_rule(variant, usize::from(index)))?
            {
                Some(hit) => hits.push(hit),
                None => return Ok(hits),
            }
        }
        bail!("hit program exceeds bounded record limit")
    }

    pub fn hits(&self, variant: usize) -> Result<Vec<HitWindow>> {
        self.hits_at(
            variant,
            self.phase(variant)?
                .hit_root
                .context("missing action hit root")?,
        )
    }

    pub fn animations_with_continuations(
        &self,
        variant: usize,
        continuations: &[(u8, i8)],
    ) -> Result<AnimationProgram> {
        self.animations.selected_with_continuations(
            self.phase(variant)?
                .animation_root
                .context("missing action animation root")?,
            continuations,
        )
    }

    pub fn commands(&self, variant: usize) -> Result<(Vec<TimedCommand>, bool)> {
        let commands = &self.phase(variant)?.commands;
        ensure!(
            commands.initialized != Some(false),
            "uninitialized action command program"
        );
        ensure!(
            commands.commands.len() < LIMIT,
            "action command program exceeds bounded record limit"
        );
        Ok((
            commands
                .commands
                .iter()
                .map(lower_command)
                .collect::<Result<_>>()?,
            commands.loops,
        ))
    }

    pub fn action(&self, variant: usize, tp: u8) -> Result<Action> {
        let phase = self.phase(variant)?;
        let (commands, loop_commands) = self.commands(variant)?;
        Ok(Action {
            duration: phase.duration,
            tp,
            animations: self.animations.selected(
                phase
                    .animation_root
                    .context("missing action animation root")?,
            )?,
            commands,
            loop_commands,
            hits: self.hits(variant)?,
        })
    }
}
