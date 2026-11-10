//! Reviewed meanings for individual operands where even a field-wide alias is
//! ambiguous. A profile requires the entire original resource's SHA-256, exact
//! native signature/value, global load address/width or exact flag-ID assignment.
//! Friendly names use
//! explicit members in the supplied definitions. Proven non-quest global roles
//! also prevent quest-stage inference when an optional alias is omitted.
//! It affects source annotations only; no original storage or encoding changes.
use super::{Context, Definitions};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
use symphonia_script::{scenario::Instruction, semantics::NativeProcedure};

#[derive(Clone, Copy)]
enum Host {
    Field,
    World,
}

impl Host {
    fn accepts(self, context: Context) -> bool {
        matches!(context, Context::Unspecified)
            || matches!(
                (self, context),
                (Self::Field, Context::Field | Context::FieldScene(_))
                    | (Self::World, Context::World)
            )
    }
}

#[derive(Clone, Copy)]
struct Entry {
    digest: [u8; 32],
    host: Host,
    pc: u32,
    native: &'static str,
    parameter: &'static str,
    value: i32,
    domain: &'static str,
}

#[derive(Default)]
pub(super) struct OperandProfiles {
    entries: BTreeMap<u32, Vec<Entry>>,
}

impl OperandProfiles {
    pub(super) fn flag_constant_domain(
        &self,
        load: &Instruction,
        literal: &Instruction,
        calculate: &Instruction,
        definitions: &Definitions,
    ) -> Option<&'static str> {
        if load.mnemonic != "load.s32"
            || !literal.mnemonic.starts_with("push.")
            || calculate.mnemonic != "calc"
            || calculate.operands != [0x10]
            || load.end_pc() != literal.pc
            || literal.end_pc() != calculate.pc
        {
            return None;
        }
        let parameter = format!("{:#x}/0x10", load.operands[0]);
        self.entries.get(&load.pc)?.iter().find_map(|entry| {
            (entry.native == "memory::flag_constant32"
                && entry.parameter == parameter
                && entry.domain == "story::StoryFlag"
                && literal.operands == [i64::from(entry.value)]
                && definitions.name(entry.domain, entry.value).is_some())
            .then_some(entry.domain)
        })
    }

    pub(super) fn global_role(&self, instruction: &Instruction) -> Option<&'static str> {
        let address = *instruction.operands.first()?;
        if instruction.mnemonic != "load.s32"
            || !(0x40..0x440).contains(&address)
            || address % 4 != 0
        {
            return None;
        }
        self.entries.get(&instruction.pc)?.iter().find_map(|entry| {
            (entry.native == "memory::global_alias"
                && entry.parameter == "address"
                && i64::from(entry.value) == address)
                .then_some(entry.domain)
        })
    }

    pub(super) fn for_bytes(bytes: &[u8], context: Context) -> Self {
        static CATALOG: OnceLock<Vec<Entry>> = OnceLock::new();
        Self::from_catalog(bytes, context, CATALOG.get_or_init(catalog))
    }

    fn from_catalog(bytes: &[u8], context: Context, catalog: &[Entry]) -> Self {
        let mut result = Self::default();
        if matches!(context, Context::Battle | Context::Skit) {
            return result;
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        for entry in catalog
            .iter()
            .filter(|entry| entry.digest == digest && entry.host.accepts(context))
        {
            result.entries.entry(entry.pc).or_default().push(*entry);
        }
        result
    }

    pub(super) fn domain(
        &self,
        pc: u32,
        native: &NativeProcedure,
        index: usize,
        value: Option<i32>,
        definitions: &Definitions,
    ) -> Option<&'static str> {
        if native.argument_types.get(index).map(String::as_str) != Some("story::StoryFlag") {
            return None;
        }
        self.entries.get(&pc)?.iter().find_map(|entry| {
            (entry.native == native.name
                && native.arguments.get(index).map(String::as_str) == Some(entry.parameter)
                && value == Some(entry.value)
                && definitions.name(entry.domain, entry.value).is_some())
            .then_some(entry.domain)
        })
    }
}

fn catalog() -> Vec<Entry> {
    let mut seen = BTreeSet::new();
    include_str!("../../data/scenario-operand-domains.tsv")
        .lines()
        .skip(1)
        .map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            assert_eq!(columns.len(), 8, "invalid operand profile row");
            assert_eq!(columns[0].len(), 64, "invalid operand profile digest");
            let mut digest = [0; 32];
            for (index, byte) in digest.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&columns[0][2 * index..2 * index + 2], 16)
                    .expect("hex operand profile digest");
            }
            let pc = u32::from_str_radix(
                columns[2]
                    .strip_prefix("0x")
                    .expect("hex operand profile PC"),
                16,
            )
            .expect("valid operand profile PC");
            assert!(
                seen.insert((digest, pc, columns[4])),
                "duplicate operand profile"
            );
            assert!(columns[6].starts_with("story::") && !columns[7].is_empty());
            Entry {
                digest,
                host: match columns[1] {
                    "field" => Host::Field,
                    "world" => Host::World,
                    _ => panic!("invalid operand profile host"),
                },
                pc,
                native: columns[3],
                parameter: columns[4],
                value: columns[5].parse().expect("integer operand profile value"),
                domain: columns[6],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{compile, decode::decompile_with_profiles};

    fn definitions(lightning: &str, actors: &str) -> Definitions {
        Definitions::from_sources(&BTreeMap::from([
            (
                "std::story".into(),
                format!(
                    "script library; pub enum StoryFlag {{ Unrelated = 999 }}
                    {lightning} {actors}"
                ),
            ),
            (
                "std::items".into(),
                "script library; pub enum ItemId { Item150 = 150 }".into(),
            ),
        ]))
        .unwrap()
    }

    fn original_definitions() -> Definitions {
        definitions(
            "pub enum DebugLightningFlag { BlueSelected = 150 }",
            "pub enum DebugActorSlotFlag { Actor32InUse = 150 }",
        )
    }

    fn fixture(body: &str) -> Vec<u8> {
        let source = format!(
            "script scenario; code_base = 4;
            data {{ word(4); pc(entry); offset(auxiliary); word(0); }}
            task entry() {{ {body} end; }}
            auxiliary: data {{ hex(\"AABB\"); }}"
        );
        compile("profile", &source, &original_definitions()).unwrap()
    }

    fn entries(bytes: &[u8]) -> Vec<Entry> {
        let digest = Sha256::digest(bytes).into();
        symphonia_script::scenario::analyze(bytes)
            .unwrap()
            .instructions
            .values()
            .filter(|i| i.mnemonic == "proc")
            .map(|i| Entry {
                digest,
                host: Host::Field,
                pc: i.pc,
                native: "set_event_bit",
                parameter: "bit",
                value: 150,
                domain: "story::DebugLightningFlag",
            })
            .collect()
    }

    fn decompile(
        bytes: &[u8],
        definitions: &Definitions,
        context: Context,
        catalog: &[Entry],
    ) -> super::super::Decompilation {
        let result = decompile_with_profiles(
            bytes,
            definitions,
            context,
            OperandProfiles::from_catalog(bytes, context, catalog),
            None,
        )
        .unwrap();
        assert_eq!(
            compile("profile", &result.source, definitions).unwrap(),
            bytes
        );
        result
    }

    #[test]
    fn identical_bits_get_operand_specific_names_from_the_supplied_definitions() {
        let bytes = fixture("native::set_event_bit(150); native::set_event_bit(150);");
        let mut catalog = entries(&bytes);
        catalog[1].domain = "story::DebugActorSlotFlag";
        for context in [
            Context::Unspecified,
            Context::Field,
            Context::FieldScene(123),
        ] {
            let result = decompile(&bytes, &original_definitions(), context, &catalog);
            assert!(
                result
                    .source
                    .contains("story::DebugLightningFlag::BlueSelected")
            );
            assert!(
                result
                    .source
                    .contains("story::DebugActorSlotFlag::Actor32InUse")
            );
        }
        let renamed = definitions(
            "pub enum DebugLightningFlag { ModdedBlueName = 150 }",
            "pub enum DebugActorSlotFlag { ModdedActorName = 150 }",
        );
        let result = decompile(&bytes, &renamed, Context::Field, &catalog);
        assert!(
            result
                .source
                .contains("story::DebugLightningFlag::ModdedBlueName")
        );
        assert!(
            result
                .source
                .contains("story::DebugActorSlotFlag::ModdedActorName")
        );
        assert!(!result.source.contains("BlueSelected"));
        assert!(!result.source.contains("Actor32InUse"));

        for (lightning, actors) in [
            ("", ""),
            (
                "pub enum DebugLightningFlag { DifferentValue = 151 }",
                "pub enum DebugActorSlotFlag { DifferentValue = 151 }",
            ),
            (
                "pub enum UnrelatedLightningFlag { BlueSelected = 150 }",
                "pub enum UnrelatedActorFlag { Actor32InUse = 150 }",
            ),
        ] {
            let result = decompile(
                &bytes,
                &definitions(lightning, actors),
                Context::Field,
                &catalog,
            );
            assert!(
                result
                    .identities
                    .iter()
                    .all(|id| id.domain == "story::StoryFlag")
            );
            assert!(result.identities.iter().all(|id| !id.symbolic));
        }
    }

    #[test]
    fn any_binary_change_or_nonfield_host_disables_the_profile() {
        let bytes = fixture("native::set_event_bit(150); native::set_event_bit(150);");
        let catalog = entries(&bytes);
        let definitions = original_definitions();
        for context in [Context::World, Context::Battle, Context::Skit] {
            let result = decompile(&bytes, &definitions, context, &catalog);
            assert!(
                result
                    .identities
                    .iter()
                    .all(|id| id.domain == "story::StoryFlag")
            );
        }
        let mut changed_data = bytes.clone();
        *changed_data.last_mut().unwrap() ^= 1;
        let changed_code = fixture("native::set_event_bit(151); native::set_event_bit(150);");
        for changed in [&changed_data, &changed_code] {
            assert!(
                OperandProfiles::from_catalog(changed, Context::Field, &catalog)
                    .entries
                    .is_empty()
            );
            let result = decompile(changed, &definitions, Context::Field, &catalog);
            assert!(
                result
                    .identities
                    .iter()
                    .all(|id| id.domain == "story::StoryFlag")
            );
            assert!(!result.source.contains("DebugLightningFlag"));
        }
    }

    #[test]
    fn world_profiles_distinguish_two_destination_meanings_of_the_same_bit() {
        let bytes = fixture("native::set_event_bit(199); native::set_event_bit(199);");
        let definitions = definitions(
            "pub enum Field0300Flag { PietroInnReminderEnabled = 199 }",
            "pub enum Field0243Flag { ExteriorHouseOfGuidance0093Selected = 199 }",
        );
        let mut catalog = entries(&bytes);
        for entry in &mut catalog {
            entry.host = Host::World;
            entry.value = 199;
        }
        catalog[0].domain = "story::Field0300Flag";
        catalog[1].domain = "story::Field0243Flag";
        for context in [Context::Unspecified, Context::World] {
            let result = decompile(&bytes, &definitions, context, &catalog);
            assert!(
                result
                    .source
                    .contains("story::Field0300Flag::PietroInnReminderEnabled")
            );
            assert!(
                result
                    .source
                    .contains("story::Field0243Flag::ExteriorHouseOfGuidance0093Selected")
            );
            assert_eq!(result.identities[0].domain, "story::Field0300Flag");
            assert_eq!(result.identities[1].domain, "story::Field0243Flag");
        }
        for context in [
            Context::Field,
            Context::FieldScene(123),
            Context::Battle,
            Context::Skit,
        ] {
            let result = decompile(&bytes, &definitions, context, &catalog);
            assert!(
                result
                    .identities
                    .iter()
                    .all(|id| id.domain == "story::StoryFlag")
            );
        }
    }

    #[test]
    fn matching_hash_and_pc_still_require_the_exact_flag_signature_and_value() {
        let bytes = fixture(
            "native::set_event_bit(150); native::set_event_bit(150);
            native::set_event_bit(150); eval native::get_item_count(150);
            native::set_event_bit(memory::i32[0x0810]);",
        );
        let mut catalog = entries(&bytes);
        catalog[0].native = "clear_event_bit";
        catalog[1].parameter = "wrong_parameter";
        catalog[2].value = 151;
        catalog[3].native = "get_item_count";
        catalog[3].parameter = "item_id";
        let result = decompile(&bytes, &original_definitions(), Context::Field, &catalog);
        assert!(!result.source.contains("DebugLightningFlag"));
        assert!(result.source.contains("items::ItemId::Item150"));
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "story::StoryFlag")
                .count(),
            3
        );
    }
    fn global_definitions(aliases: &str) -> Definitions {
        Definitions::from_sources(&BTreeMap::from([(
            "std::story".into(),
            format!(
                "script library; pub enum GlobalId {{ Ice = 42, Herb = 59 }}
             pub enum CelsiusPactStage {{ QuestPhase = 1000 }}
             pub enum ManaLeafHerbStage {{ HerbPhase = 1000 }} {aliases}"
            ),
        )]))
        .unwrap()
    }

    fn global_fixture() -> Vec<u8> {
        fixture(
            "eval (memory::i32[0x00E8] == 1000);
            eval (memory::i32[0x0810] = memory::i32[0x00E8]);
            eval (memory::i32[0x0810] == 1000);
            eval (memory::i32[0x00E8] == 1000);
            eval (memory::i32[0x012C] = 7);
            eval (memory::i32[0x012C] == 1000);",
        )
    }

    fn global_entries(bytes: &[u8]) -> Vec<Entry> {
        let digest = Sha256::digest(bytes).into();
        let mut counters = 0;
        let mut rooms = 0;
        symphonia_script::scenario::analyze(bytes)
            .unwrap()
            .instructions
            .values()
            .filter(|i| i.mnemonic == "load.s32")
            .filter(|i| match i.operands[0] {
                0xe8 => {
                    counters += 1;
                    counters <= 2
                }
                0x12c => {
                    rooms += 1;
                    rooms == 1
                }
                _ => false,
            })
            .map(|i| Entry {
                digest,
                host: Host::Field,
                pc: i.pc,
                native: "memory::global_alias",
                parameter: "address",
                value: i.operands[0] as i32,
                domain: "story::PalmacostaGlobalId",
            })
            .collect()
    }

    #[test]
    fn global_roles_keep_shared_words_and_copies_out_of_unrelated_quest_domains() {
        let bytes = global_fixture();
        let catalog = global_entries(&bytes);
        assert_eq!(catalog.len(), 3);
        for (aliases, expected) in [
            (
                "pub enum PalmacostaGlobalId { WarpBattleCount = 42, WarpRoomState = 59 }",
                Some("WarpBattleCount"),
            ),
            (
                "pub enum PalmacostaGlobalId { CustomCounter = 42, CustomRoom = 59 }",
                Some("CustomCounter"),
            ),
            (
                "pub enum PalmacostaGlobalId { MovedCounter = 43, MovedRoom = 60 }",
                None,
            ),
            ("", None),
        ] {
            let definitions = global_definitions(aliases);
            let result = decompile(&bytes, &definitions, Context::Field, &catalog);
            assert_eq!(
                result
                    .source
                    .matches("CelsiusPactStage::QuestPhase")
                    .count(),
                1,
                "{}",
                result.source
            );
            assert_eq!(
                result
                    .source
                    .matches("ManaLeafHerbStage::HerbPhase")
                    .count(),
                1
            );
            assert!(result.source.contains("memory::i32[0x0810] == 1000"));
            let uses: Vec<_> = result
                .identities
                .iter()
                .filter(|id| id.native == "memory::global_alias")
                .collect();
            assert_eq!(uses.len(), if expected.is_some() { 3 } else { 0 });
            if let Some(expected) = expected {
                assert!(result.source.contains(&format!(
                    "globals[story::PalmacostaGlobalId::{expected}] == 1000"
                )));
            } else {
                assert!(
                    result
                        .source
                        .contains("globals[story::GlobalId::Ice] == 1000")
                );
                assert!(!result.source.contains("PalmacostaGlobalId::"));
            }
        }
    }

    #[test]
    fn global_roles_require_original_hash_host_word_width_address_and_parameter() {
        let bytes = global_fixture();
        let definitions =
            global_definitions("pub enum PalmacostaGlobalId { Counter = 42, Room = 59 }");
        let catalog = global_entries(&bytes);
        for context in [
            Context::Unspecified,
            Context::Field,
            Context::FieldScene(206),
        ] {
            let result = decompile(&bytes, &definitions, context, &catalog);
            assert_eq!(
                result
                    .identities
                    .iter()
                    .filter(|id| id.native == "memory::global_alias")
                    .count(),
                3
            );
        }
        for context in [Context::Battle, Context::Skit, Context::World] {
            let result = decompile(&bytes, &definitions, context, &catalog);
            assert!(!result.source.contains("PalmacostaGlobalId::"));
        }
        let mut changed = bytes.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(
            !decompile(&changed, &definitions, Context::Field, &catalog)
                .source
                .contains("PalmacostaGlobalId::")
        );
        let profile = OperandProfiles::from_catalog(&bytes, Context::Field, &catalog);
        let analysis = symphonia_script::scenario::analyze(&bytes).unwrap();
        let instruction = analysis.instructions[&catalog[0].pc].clone();
        assert!(profile.global_role(&instruction).is_some());
        let mut narrow = instruction.clone();
        narrow.mnemonic = "load.s16";
        assert!(profile.global_role(&narrow).is_none());
        let mut other_address = instruction;
        other_address.operands[0] += 4;
        assert!(profile.global_role(&other_address).is_none());
        let mut wrong = catalog.clone();
        wrong[0].parameter = "not_address";
        wrong[1].value += 4;
        wrong[2].native = "memory::load32";
        assert!(
            !decompile(&bytes, &definitions, Context::Field, &wrong)
                .source
                .contains("PalmacostaGlobalId::")
        );
    }
    fn flag_request_entries(bytes: &[u8]) -> Vec<Entry> {
        let digest = Sha256::digest(bytes).into();
        let analysis = symphonia_script::scenario::analyze(bytes).unwrap();
        analysis
            .instructions
            .values()
            .filter(|i| i.mnemonic == "load.s32" && i.operands == [0x810])
            .filter(|i| {
                analysis
                    .instructions
                    .get(&i.end_pc())
                    .is_some_and(|lit| lit.mnemonic.starts_with("push.") && lit.operands == [150])
            })
            .map(|i| Entry {
                digest,
                host: Host::Field,
                pc: i.pc,
                native: "memory::flag_constant32",
                parameter: "0x810/0x10",
                value: 150,
                domain: "story::StoryFlag",
            })
            .collect()
    }

    #[test]
    fn flag_request_constants_name_only_exact_profiled_assignments_and_selected_members() {
        let bytes = fixture(
            "eval (memory::i32[0x0810] = s32(150)); eval (memory::i32[0x0814] = 150);
            eval (memory::i32[0x0810] == 150); eval (memory::i32[0x0810] = 0);
            eval (memory::i16[0x0810] = 150); eval 150;",
        );
        let catalog = flag_request_entries(&bytes);
        assert_eq!(catalog.len(), 2); // assignment and comparison, only assignment is consumed
        for (member, expected) in [
            ("NamedFlag = 150", Some("NamedFlag")),
            ("ModFlag = 150", Some("ModFlag")),
            ("MovedFlag = 151", None),
            ("", None),
        ] {
            let definitions = Definitions::from_sources(&BTreeMap::from([(
                "std::story".into(),
                format!("script library;pub enum StoryFlag{{ {member} }}"),
            )]))
            .unwrap();
            let result = decompile(&bytes, &definitions, Context::Field, &catalog);
            let uses: Vec<_> = result
                .identities
                .iter()
                .filter(|id| id.native == "memory::flag_constant32")
                .collect();
            assert_eq!(uses.len(), usize::from(expected.is_some()));
            if let Some(name) = expected {
                assert!(result.source.contains(&format!(
                    "memory::i32[0x0810] = s32(story::StoryFlag::{name})"
                )));
            } else {
                assert!(result.source.contains("memory::i32[0x0810] = s32(150)"));
            }
            for raw in [
                "memory::i32[0x0814] = 150",
                "memory::i32[0x0810] == 150",
                "memory::i32[0x0810] = 0",
                "memory::i16[0x0810] = 150",
                "eval 150;",
            ] {
                assert!(result.source.contains(raw), "{raw}\n{}", result.source);
            }
        }
    }

    #[test]
    fn flag_request_profiles_require_hash_host_address_width_value_and_assignment() {
        let bytes = fixture(
            "eval(memory::i32[0x0810]=150);eval(memory::i32[0x0810]=150);eval(memory::i32[0x0810]=150);",
        );
        let catalog = flag_request_entries(&bytes);
        let defs = Definitions::from_sources(&BTreeMap::from([(
            "std::story".into(),
            "script library;pub enum StoryFlag{NamedFlag=150}".into(),
        )]))
        .unwrap();
        for context in [
            Context::Field,
            Context::FieldScene(493),
            Context::Unspecified,
        ] {
            assert_eq!(
                decompile(&bytes, &defs, context, &catalog)
                    .identities
                    .iter()
                    .filter(|id| id.native == "memory::flag_constant32")
                    .count(),
                3
            );
        }
        for context in [Context::Battle, Context::Skit, Context::World] {
            assert!(
                !decompile(&bytes, &defs, context, &catalog)
                    .source
                    .contains("StoryFlag::NamedFlag")
            );
        }
        let mut changed = bytes.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(
            !decompile(&changed, &defs, Context::Field, &catalog)
                .source
                .contains("StoryFlag::NamedFlag")
        );
        let mut wrong = catalog.clone();
        wrong[0].parameter = "0x814/0x10";
        wrong[1].value = 151;
        wrong[2].native = "memory::constant32";
        assert!(
            !decompile(&bytes, &defs, Context::Field, &wrong)
                .source
                .contains("StoryFlag::NamedFlag")
        );
        let profiles = OperandProfiles::from_catalog(&bytes, Context::Field, &catalog);
        let a = symphonia_script::scenario::analyze(&bytes).unwrap();
        let load = a.instructions[&catalog[0].pc].clone();
        let lit = a.instructions[&load.end_pc()].clone();
        let calc = a.instructions[&lit.end_pc()].clone();
        assert!(
            profiles
                .flag_constant_domain(&load, &lit, &calc, &defs)
                .is_some()
        );
        let mut narrow = load.clone();
        narrow.mnemonic = "load.s16";
        assert!(
            profiles
                .flag_constant_domain(&narrow, &lit, &calc, &defs)
                .is_none()
        );
        let mut different = calc.clone();
        different.operands = vec![0x20];
        assert!(
            profiles
                .flag_constant_domain(&load, &lit, &different, &defs)
                .is_none()
        );
        let mut separated = lit;
        separated.pc += 1;
        assert!(
            profiles
                .flag_constant_domain(&load, &separated, &calc, &defs)
                .is_none()
        );
    }
}
