//! Recovered calculator semantics and data-driven native-call naming.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorForm {
    Drop,
    Postfix,
    Prefix,
    Noop,
    Index,
    Assignment,
    Binary,
    CompoundAssignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorSpec {
    pub opcode: u8,
    pub symbol: &'static str,
    pub form: OperatorForm,
    pub inputs: u8,
    pub stack_delta: i8,
    pub writes_reference: bool,
}

/// Decode the stack effects and argument layout of a calculator opcode.
#[must_use]
pub const fn calculator(opcode: u8) -> Option<CalculatorSpec> {
    let (symbol, form, inputs, stack_delta, writes_reference) = match opcode {
        0x00 => ("drop", OperatorForm::Drop, 1, -1, false),
        0x01 => ("post++", OperatorForm::Postfix, 1, 0, true),
        0x02 => ("post--", OperatorForm::Postfix, 1, 0, true),
        0x03 => ("pre++", OperatorForm::Prefix, 1, 0, true),
        0x04 => ("pre--", OperatorForm::Prefix, 1, 0, true),
        0x05 => ("-", OperatorForm::Prefix, 1, 0, false),
        0x06 => ("+", OperatorForm::Noop, 1, 0, false),
        0x07 => ("~", OperatorForm::Prefix, 1, 0, false),
        0x08 => ("!", OperatorForm::Prefix, 1, 0, false),
        0x0f => ("[]", OperatorForm::Index, 2, -1, false),
        0x10 => ("=", OperatorForm::Assignment, 2, -1, true),
        0x11 => ("+=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x12 => ("-=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x13 => ("*=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x14 => ("/=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x15 => ("%=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x16 => ("&=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x17 => ("|=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x18 => ("^=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x19 => ("<<=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x1a => (">>=", OperatorForm::CompoundAssignment, 2, -1, true),
        0x20 => ("==", OperatorForm::Binary, 2, -1, false),
        0x21 => ("!=", OperatorForm::Binary, 2, -1, false),
        0x22 => ("<=", OperatorForm::Binary, 2, -1, false),
        0x23 => (">=", OperatorForm::Binary, 2, -1, false),
        0x24 => (">", OperatorForm::Binary, 2, -1, false),
        0x25 => ("<", OperatorForm::Binary, 2, -1, false),
        0x26 => ("&&", OperatorForm::Binary, 2, -1, false),
        0x27 => ("||", OperatorForm::Binary, 2, -1, false),
        0x31 => ("+", OperatorForm::Binary, 2, -1, false),
        0x32 => ("-", OperatorForm::Binary, 2, -1, false),
        0x33 => ("*", OperatorForm::Binary, 2, -1, false),
        0x34 => ("/", OperatorForm::Binary, 2, -1, false),
        0x35 => ("%", OperatorForm::Binary, 2, -1, false),
        0x36 => ("&", OperatorForm::Binary, 2, -1, false),
        0x37 => ("|", OperatorForm::Binary, 2, -1, false),
        0x38 => ("^", OperatorForm::Binary, 2, -1, false),
        0x39 => ("<<", OperatorForm::Binary, 2, -1, false),
        0x3a => (">>", OperatorForm::Binary, 2, -1, false),
        _ => return None,
    };
    Some(CalculatorSpec {
        opcode,
        symbol,
        form,
        inputs,
        stack_delta,
        writes_reference,
    })
}

/// Resolve the stable source mnemonic for a recovered calculator operation.
#[must_use]
pub fn calculator_by_name(name: &str) -> Option<CalculatorSpec> {
    let opcode = match name {
        "expression_end" => 0x00,
        "post_increment" => 0x01,
        "post_decrement" => 0x02,
        "pre_increment" => 0x03,
        "pre_decrement" => 0x04,
        "negate" => 0x05,
        "positive" => 0x06,
        "bitwise_not" => 0x07,
        "logical_not" => 0x08,
        "indexed_reference" => 0x0f,
        "assign" => 0x10,
        "add_assign" => 0x11,
        "subtract_assign" => 0x12,
        "multiply_assign" => 0x13,
        "divide_assign" => 0x14,
        "remainder_assign" => 0x15,
        "and_assign" => 0x16,
        "or_assign" => 0x17,
        "xor_assign" => 0x18,
        "shift_left_assign" => 0x19,
        "shift_right_assign" => 0x1a,
        "equal" => 0x20,
        "not_equal" => 0x21,
        "less_equal" => 0x22,
        "greater_equal" => 0x23,
        "greater" => 0x24,
        "less" => 0x25,
        "logical_and" => 0x26,
        "logical_or" => 0x27,
        "add" => 0x31,
        "subtract" => 0x32,
        "multiply" => 0x33,
        "divide" => 0x34,
        "remainder" => 0x35,
        "bitwise_and" => 0x36,
        "bitwise_or" => 0x37,
        "bitwise_xor" => 0x38,
        "shift_left" => 0x39,
        "shift_right" => 0x3a,
        _ => return None,
    };
    calculator(opcode)
}

/// Return the stable source mnemonic for a calculator opcode.
///
/// This is the inverse of [`calculator_by_name`].  Keeping this mapping next
/// to the decoder's opcode table lets the exact-layout decompiler emit names
/// without changing the serialized instruction stream.
#[must_use]
pub fn calculator_name(opcode: u8) -> Option<&'static str> {
    Some(match opcode {
        0x00 => "expression_end",
        0x01 => "post_increment",
        0x02 => "post_decrement",
        0x03 => "pre_increment",
        0x04 => "pre_decrement",
        0x05 => "negate",
        0x06 => "positive",
        0x07 => "bitwise_not",
        0x08 => "logical_not",
        0x0f => "indexed_reference",
        0x10 => "assign",
        0x11 => "add_assign",
        0x12 => "subtract_assign",
        0x13 => "multiply_assign",
        0x14 => "divide_assign",
        0x15 => "remainder_assign",
        0x16 => "and_assign",
        0x17 => "or_assign",
        0x18 => "xor_assign",
        0x19 => "shift_left_assign",
        0x1a => "shift_right_assign",
        0x20 => "equal",
        0x21 => "not_equal",
        0x22 => "less_equal",
        0x23 => "greater_equal",
        0x24 => "greater",
        0x25 => "less",
        0x26 => "logical_and",
        0x27 => "logical_or",
        0x31 => "add",
        0x32 => "subtract",
        0x33 => "multiply",
        0x34 => "divide",
        0x35 => "remainder",
        0x36 => "bitwise_and",
        0x37 => "bitwise_or",
        0x38 => "bitwise_xor",
        0x39 => "shift_left",
        0x3a => "shift_right",
        _ => return None,
    })
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProcedure {
    pub opcode: u8,
    pub name: String,
    #[serde(default)]
    pub handler: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub argument_types: Vec<String>,
    #[serde(default)]
    pub returns_value: Option<bool>,
    #[serde(default)]
    pub result_type: Option<String>,
    #[serde(default)]
    pub yields: bool,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub arity_evidence: String,
    #[serde(default)]
    pub return_evidence: String,
    #[serde(default)]
    pub control_flow: bool,
    #[serde(default = "default_true")]
    pub authoring_available: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    schema_version: u32,
    game: String,
    #[serde(default)]
    dispatch_entry_count: Option<usize>,
    #[serde(default)]
    procedures: Vec<NativeProcedure>,
}

#[derive(Debug)]
pub struct NativeRegistry {
    pub game: String,
    calls: BTreeMap<u8, NativeProcedure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlSurfaceEntry {
    pub opcode: u8,
    pub name: String,
    pub handler: String,
    pub arguments: Vec<String>,
    pub argument_types: Vec<String>,
    pub returns_value: Option<bool>,
    pub result_type: Option<String>,
    pub yields: bool,
    pub confidence: String,
    pub domain: &'static str,
    pub source: String,
    pub notes: String,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("invalid native registry JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported native registry schema version {0}")]
    Schema(u32),
    #[error("duplicate native procedure opcode 0x{0:02X}")]
    Duplicate(u8),
    #[error("native procedure name {0:?} is not a valid SymphoniaScript identifier")]
    InvalidName(String),
    #[error("native procedure 0x{opcode:02X} has {arguments} arguments but {types} argument types")]
    ArgumentTypeCount {
        opcode: u8,
        arguments: usize,
        types: usize,
    },
    #[error("native registry declares {declared} dispatch entries but contains {actual}")]
    DispatchCount { declared: usize, actual: usize },
}

impl NativeRegistry {
    /// Load the checked-in registry recovered for the active retail revision.
    ///
    /// # Panics
    ///
    /// Panics only if the registry shipped in this crate fails its own schema
    /// validation, which is also covered by the test suite.
    #[must_use]
    pub fn gqseaf() -> Self {
        Self::from_json(include_str!("../data/GQSEAF/native_procedures.json"))
            .expect("the checked-in native procedure registry must be valid")
    }

    /// Load and validate a game-specific native procedure registry.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid JSON, schema versions, opcodes, or names.
    pub fn from_json(source: &str) -> Result<Self, RegistryError> {
        let document: RegistryDocument = serde_json::from_str(source)?;
        if !matches!(document.schema_version, 1 | 2) {
            return Err(RegistryError::Schema(document.schema_version));
        }
        if let Some(declared) = document.dispatch_entry_count
            && declared != document.procedures.len()
        {
            return Err(RegistryError::DispatchCount {
                declared,
                actual: document.procedures.len(),
            });
        }
        let mut calls = BTreeMap::new();
        for call in document.procedures {
            if !valid_identifier(&call.name) {
                return Err(RegistryError::InvalidName(call.name));
            }
            if !call.argument_types.is_empty() && call.argument_types.len() != call.arguments.len()
            {
                return Err(RegistryError::ArgumentTypeCount {
                    opcode: call.opcode,
                    arguments: call.arguments.len(),
                    types: call.argument_types.len(),
                });
            }
            let opcode = call.opcode;
            if calls.insert(opcode, call).is_some() {
                return Err(RegistryError::Duplicate(opcode));
            }
        }
        Ok(Self {
            game: document.game,
            calls,
        })
    }

    #[must_use]
    pub fn get(&self, opcode: u8) -> Option<&NativeProcedure> {
        self.calls.get(&opcode)
    }

    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<&NativeProcedure> {
        if let Some(call) = self.calls.values().find(|call| call.name == name) {
            return Some(call);
        }
        // Keep old decompilations source-compatible after an opcode receives
        // an evidence-backed semantic name.  The numeric spelling is an
        // intentionally supported escape hatch, not the preferred DSL form.
        let opcode = name
            .strip_prefix("native_")
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())?;
        self.calls.get(&opcode)
    }

    pub fn iter(&self) -> impl Iterator<Item = &NativeProcedure> {
        self.calls.values()
    }

    /// Return a stable, evidence-oriented view of what each native opcode can
    /// control. Domains are intentionally broad; unresolved argument meaning
    /// remains visible in the original signature and notes.
    #[must_use]
    pub fn control_surface(&self) -> Vec<ControlSurfaceEntry> {
        self.iter()
            .map(|call| ControlSurfaceEntry {
                opcode: call.opcode,
                name: call.name.clone(),
                handler: call.handler.clone(),
                arguments: call.arguments.clone(),
                argument_types: call.argument_types.clone(),
                returns_value: call.returns_value,
                result_type: call.result_type.clone(),
                yields: call.yields,
                confidence: call.confidence.clone(),
                domain: domain_for(call),
                source: call.source.clone(),
                notes: call.notes.clone(),
            })
            .collect()
    }
}

fn domain_for(call: &NativeProcedure) -> &'static str {
    let name = call.name.as_str();
    if call.control_flow {
        return "interpreter.control_flow";
    }
    if name.contains("dialogue")
        || name.contains("message")
        || name.contains("choice")
        || name == "yield_command"
    {
        return "dialogue_and_yield";
    }
    if name.contains("camera") {
        return "camera";
    }
    if name.contains("actor") || name.contains("object") || name.contains("animation") {
        return "actors_and_objects";
    }
    if name.contains("field")
        || name.contains("stage")
        || name.contains("encounter")
        || name.contains("transition")
    {
        return "field_and_stage";
    }
    if name.contains("sound") || name.contains("audio") {
        return "audio";
    }
    if name.contains("item") || name.contains("inventory") {
        return "inventory";
    }
    if name.contains("event_bit") || name.contains("global") || name.contains("counter") {
        return "persistent_state";
    }
    if name.contains("input") {
        return "input";
    }
    if name.contains("resource") {
        return "resource_lifecycle";
    }
    "unknown_native_side_effect"
}

fn valid_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
