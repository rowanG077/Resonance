//! Exhaustive source inventory. Recovered records are not executable-test results.
use super::{
    action_program::CommandProgram,
    actions::{AnimationProgram, HitRule, HitWindow, TechniqueProperties},
};
use crate::menu_data::TECHNIQUE_COUNT;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ops::Range};

pub const SPELL_NATIVE_IDS: Range<u16> = 200..300;
/// Includes the explicitly empty first dispatch slot.
pub const COMBINED_NATIVE_IDS: Range<u16> = 300..319;
pub const NATIVE_IDS: Range<u16> = SPELL_NATIVE_IDS.start..COMBINED_NATIVE_IDS.end;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArteInventory {
    pub artes: Vec<ArteRecord>,
    pub party: Vec<PartyArteInventory>,
    pub martial: Vec<MartialArteInventory>,
    /// Includes empty dispatch slots and aliases, not only player-owned spells.
    pub native: Vec<NativeArteInventory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArteRecord {
    pub id: u16,
    pub native_id: u16,
    pub name: String,
    pub owners: Vec<u8>,
    pub cost: ArteCost,
    pub properties: TechniqueProperties,
    pub targeting: ArteTargetMetadata,
    pub cast_time_adjustment: i16,
    pub category: u8,
    pub element: u8,
    pub route: LearningRoute,
    pub level: u16,
    pub prerequisite: Option<u16>,
    pub strike_successor: Option<u16>,
    pub technical_successor: Option<u16>,
    pub alternatives: Vec<u16>,
    pub loads_resource: bool,
    /// Preserved named metadata for flags whose additional policies need auditing.
    pub source_flags: u32,
    pub implementation: ArteImplementation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "amount", rename_all = "snake_case")]
pub enum ArteCost {
    Tp(u8),
    MaximumTpPercent(u8),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningRoute {
    Either,
    Technical,
    Strike,
    Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArteTargetMetadata {
    /// Authored AI spread selector. Individual values still need a full policy audit.
    pub spread_class: u8,
    pub condition_mask: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "native_id", rename_all = "snake_case")]
pub enum ArteImplementation {
    Dummy,
    Martial(u16),
    Native(u16),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyArteInventory {
    pub character: u8,
    /// Preserves source list order, including shared Kratos/Zelos techniques.
    pub artes: Vec<u16>,
    pub normals: Vec<NormalBranchInventory>,
    pub casting_voices: Vec<CastingVoiceBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastingVoiceBinding {
    pub native_id: u16,
    pub begin: u16,
    pub release: Option<u16>,
    pub begin_remaining_ticks: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalBranchInventory {
    pub selection: u8,
    pub bundle: u8,
    pub followup_directions: u8,
    pub fallback: Option<u8>,
    pub duration: u16,
    pub recovery_ticks: u16,
    pub combo_first: u16,
    pub combo_second: Option<u16>,
    pub buffer_until: u8,
    pub recovery_animation: Option<u8>,
    pub recovery_rate: f32,
    pub reach: u16,
    pub airborne_reach: u16,
    pub startup_effect: Option<u16>,
    pub programs: ActionPrograms,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArteBundle {
    pub native_id: u16,
    pub phases: Recovery<Vec<ArtePhaseInventory>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MartialArteInventory {
    pub native_id: u16,
    pub dispatch: NativeDispatch,
    pub phases: Recovery<Vec<ArtePhaseInventory>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtePhaseInventory {
    pub phase: u8,
    /// Zero duration is retained; native callbacks may still consult the phase.
    pub duration: u16,
    pub recovery_ticks: u16,
    pub buffer_until: u16,
    pub combo_at: u16,
    pub startup_effect: Option<u16>,
    /// Native arte handlers may use these directly without a hit-window program.
    pub hit_rules: SourceProgram<Vec<HitRule>>,
    pub programs: ActionPrograms,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPrograms {
    pub animations: SourceProgram<AnimationProgram>,
    pub commands: SourceProgram<CommandProgram>,
    pub hits: SourceProgram<Vec<HitWindow>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceProgram<T> {
    pub source: ProgramSource,
    pub recovery: Recovery<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgramSource {
    Rel {
        section: RelSection,
        offset: u32,
    },
    BattleUsual {
        member: u8,
        record: u16,
        offset: u32,
    },
    MagicArchive {
        native_id: u16,
        offset: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelSection {
    Text,
    Rodata,
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum Recovery<T> {
    Recovered(T),
    /// A source table is explicitly empty; native code supplies the behavior.
    Absent,
    Unresolved {
        stage: RecoveryStage,
        diagnostic: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStage {
    Bundle,
    Animations,
    Commands,
    Hits,
    CallbackTable,
    ExternalResource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeArteInventory {
    pub native_id: u16,
    pub dispatch: NativeDispatch,
    pub bundle: ArteBundle,
    pub resource: Recovery<Option<ArteResource>>,
    pub assets: Recovery<Option<NativeAssetRequirements>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeAssetRequirements {
    pub effect_program_offset: u32,
    pub texture_archive_offset: Option<u32>,
    pub models: Vec<NativeModelRequirement>,
    pub projectile_recipes_offset: Option<u32>,
    pub action_bundle_offset: Option<u32>,
    /// Native callbacks also receive these named slots; their semantics need auditing.
    pub callback_resources: Vec<CallbackResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeModelRequirement {
    pub slot: u8,
    pub model_offset: u32,
    pub outline_offset: Option<u32>,
    pub animations: Vec<CallbackResource>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CallbackResource {
    pub slot: u8,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeDispatch {
    Absent,
    Callbacks {
        source: ProgramSource,
        branches: Vec<NativeCallback>,
        /// Relocations enumerate entry states, not branches inside their bodies.
        control_flow: NativeControlFlow,
    },
    Unresolved {
        diagnostic: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeCallback {
    pub state: u8,
    pub handler: u32,
    pub recovery: CallbackRecovery,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackRecovery {
    BodyAndRequirementsUnresolved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeControlFlow {
    InternalBranchesUnresolved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ArteResource {
    pub archive: ArteArchive,
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArteArchive {
    Magic,
}

impl ArteInventory {
    /// Structural coverage only: unresolved records are deliberately valid inventory.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.artes.len() == TECHNIQUE_COUNT,
            "incomplete arte ID domain"
        );
        ensure!(self.party.len() == 9, "incomplete party arte inventory");
        let martial: BTreeSet<_> = self.martial.iter().map(|b| b.native_id).collect();
        ensure!(
            martial.len() == self.martial.len(),
            "duplicate martial bundle"
        );
        ensure!(
            self.native.len() == NATIVE_IDS.len(),
            "incomplete native dispatch domain"
        );
        for (index, native) in self.native.iter().enumerate() {
            ensure!(
                native.native_id == NATIVE_IDS.start + index as u16
                    && native.bundle.native_id == native.native_id,
                "invalid native dispatch identity"
            );
            validate_dispatch(&native.dispatch)?;
            if native.native_id == COMBINED_NATIVE_IDS.start {
                ensure!(
                    matches!(native.dispatch, NativeDispatch::Absent)
                        && matches!(native.bundle.phases, Recovery::Absent)
                        && matches!(native.resource, Recovery::Recovered(None))
                        && matches!(native.assets, Recovery::Recovered(None)),
                    "invalid empty combined dispatch slot"
                );
            } else {
                native.bundle.validate()?;
            }
        }
        for bundle in &self.martial {
            validate_phases(&bundle.phases)?;
            validate_dispatch(&bundle.dispatch)?;
        }
        for (index, party) in self.party.iter().enumerate() {
            ensure!(
                party.character as usize == index + 1 && party.normals.len() == 7,
                "incomplete party normal branches"
            );
            ensure!(
                party.artes.len() <= 40
                    && party.artes.iter().copied().collect::<BTreeSet<_>>().len()
                        == party.artes.len(),
                "invalid party arte list"
            );
            for (selection, branch) in party.normals.iter().enumerate() {
                ensure!(
                    branch.selection as usize == selection
                        && branch.recovery_rate.is_finite()
                        && branch.fallback.is_none_or(|id| id < 7),
                    "invalid normal branch"
                );
            }
            for &id in &party.artes {
                ensure!(
                    self.artes
                        .get(id as usize)
                        .is_some_and(|a| a.owners.contains(&party.character)),
                    "party arte ownership mismatch"
                );
            }
        }
        for (index, arte) in self.artes.iter().enumerate() {
            ensure!(
                arte.id as usize == index && (!arte.name.is_empty() || arte.owners.is_empty()),
                "invalid arte identity"
            );
            ensure!(
                arte.owners.iter().all(|c| self
                    .party
                    .iter()
                    .any(|p| p.character == *c && p.artes.contains(&arte.id))),
                "arte owner mismatch"
            );
            match arte.implementation {
                ArteImplementation::Dummy => {
                    ensure!(arte.id == 0 && arte.native_id == 0, "invalid dummy arte")
                }
                ArteImplementation::Martial(id) => ensure!(
                    id == arte.native_id && martial.contains(&id),
                    "missing martial bundle"
                ),
                ArteImplementation::Native(id) => ensure!(
                    id == arte.native_id && NATIVE_IDS.contains(&id),
                    "missing native dispatch"
                ),
            }
        }
        Ok(())
    }
}

impl ArteBundle {
    fn validate(&self) -> Result<()> {
        validate_phases(&self.phases)
    }
}

fn validate_phases(phases: &Recovery<Vec<ArtePhaseInventory>>) -> Result<()> {
    ensure!(
        !matches!(phases, Recovery::Absent),
        "missing bundle without recovery diagnostic"
    );
    if let Recovery::Recovered(phases) = phases {
        ensure!(
            phases.len() == 4
                && phases
                    .iter()
                    .enumerate()
                    .all(|(i, p)| p.phase as usize == i),
            "incomplete arte phase inventory"
        );
    }
    Ok(())
}

fn validate_dispatch(dispatch: &NativeDispatch) -> Result<()> {
    if let NativeDispatch::Callbacks { branches, .. } = dispatch {
        ensure!(
            !branches.is_empty()
                && branches
                    .iter()
                    .enumerate()
                    .all(
                        |(index, branch)| branch.state as usize == index && branch.handler % 4 == 0
                    ),
            "invalid callback state inventory"
        );
    }
    Ok(())
}
