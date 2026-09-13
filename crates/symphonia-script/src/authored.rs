//! Engine-independent types for readable scripts and their compiled programs.
//! Values occupy 32-bit slots; floats use IEEE bits and messages hold immutable
//! module indices plus bounded substitution values.
use crate::Op;
use std::collections::BTreeMap;

pub const VALUE_SLOT_LIMIT: usize = 1024;
pub const LOCAL_SLOT_LIMIT: usize = 4096;
pub const CALL_FRAME_LIMIT: usize = 64;
pub const MAX_MESSAGE_ARGUMENTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextReferenceKind {
    Character,
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeField {
    pub name: &'static str,
    pub ty: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    I32,
    F32,
    Bool,
    Ticks,
    /// Interned immutable UTF-8 string, indexed independently of display messages.
    String,
    Message,
    Handle(&'static str),
    Asset(&'static str),
    Record {
        name: &'static str,
        fields: &'static [NativeField],
    },
    Array {
        element: &'static Type,
        len: u16,
    },
    TextReference {
        name: &'static str,
        kind: TextReferenceKind,
    },
    /// Read-only host view. Its ordinary count/get natives validate lifetime and bounds.
    Collection {
        name: &'static str,
        element: &'static Type,
        count: u8,
        get: u8,
    },
}

impl Type {
    /// Messages hold an immutable resource ID and bounded, by-value substitutions.
    pub const fn slots(self) -> usize {
        match self {
            Self::Message => 1 + MAX_MESSAGE_ARGUMENTS,
            Self::Record { fields, .. } => {
                let mut total = 0usize;
                let mut index = 0;
                while index < fields.len() {
                    total = total.saturating_add(fields[index].ty.slots());
                    index += 1;
                }
                total
            }
            Self::Array { element, len } => element.slots().saturating_mul(len as usize),
            _ => 1,
        }
    }
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::Handle(name)
            | Self::Asset(name)
            | Self::Record { name, .. }
            | Self::TextReference { name, .. }
            | Self::Collection { name, .. } => Some(name),
            _ => None,
        }
    }
    pub const fn aggregate(self) -> bool {
        matches!(self, Self::Record { .. } | Self::Array { .. })
    }
    pub const fn contains_message(self) -> bool {
        match self {
            Self::Message => true,
            Self::Array { element, .. } => element.contains_message(),
            Self::Record { fields, .. } => {
                let mut index = 0;
                while index < fields.len() {
                    if fields[index].ty.contains_message() {
                        return true;
                    }
                    index += 1;
                }
                false
            }
            _ => false,
        }
    }
}

/// One declaration supplies both the compiler API and checked host registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeDeclaration {
    pub name: &'static str,
    pub opcode: u8,
    pub parameters: &'static [Type],
    pub result: Option<Type>,
    pub suspends: bool,
}
impl NativeDeclaration {
    pub const fn argument_slots(self) -> usize {
        let mut index = 0;
        let mut total = 0usize;
        while index < self.parameters.len() {
            total = total.saturating_add(self.parameters[index].slots());
            index += 1;
        }
        total
    }
}

pub fn validate_natives(natives: &[NativeDeclaration]) -> Result<(), String> {
    let mut by_id = BTreeMap::new();
    let mut names = std::collections::BTreeSet::new();
    let mut types = BTreeMap::new();
    for native in natives {
        for ty in native.parameters.iter().copied().chain(native.result) {
            validate_type(ty, &mut Vec::new(), &mut types)?;
        }
        if native.argument_slots() > 64 {
            return Err(format!(
                "native '{}' exceeds 64 argument slots",
                native.name
            ));
        }
        if native.result.is_some_and(Type::contains_message) {
            return Err(format!(
                "native '{}' cannot return a Message; construct messages in source",
                native.name
            ));
        }
        if native.suspends && native.result.is_some_and(Type::aggregate) {
            return Err(format!(
                "native '{}' cannot suspend with an aggregate result",
                native.name
            ));
        }
        if by_id.insert(native.opcode, native).is_some() || !names.insert(native.name) {
            return Err(format!("duplicate native declaration '{}'", native.name));
        }
    }
    for ty in types.into_values() {
        if let Type::Collection {
            name,
            element,
            count,
            get,
        } = ty
        {
            if count == get
                || element.slots() != 1
                || element.aggregate()
                || matches!(element, Type::Collection { .. })
            {
                return Err(format!(
                    "collection '{name}' has an invalid element or accessor collision"
                ));
            }
            let valid_count = by_id.get(&count).is_some_and(|native| {
                !native.suspends && native.parameters == [ty] && native.result == Some(Type::I32)
            });
            let valid_get = by_id.get(&get).is_some_and(|native| {
                !native.suspends
                    && native.parameters == [ty, Type::I32]
                    && native.result == Some(*element)
            });
            if !valid_count || !valid_get {
                return Err(format!(
                    "collection '{name}' requires count(collection) -> i32 and get(collection, i32) -> element without suspension"
                ));
            }
        }
    }
    Ok(())
}

fn validate_type(
    ty: Type,
    path: &mut Vec<Type>,
    types: &mut BTreeMap<&'static str, Type>,
) -> Result<(), String> {
    if path.len() >= CALL_FRAME_LIMIT
        || ty
            .name()
            .is_some_and(|name| path.iter().any(|parent| parent.name() == Some(name)))
    {
        return Err("recursive or excessively nested native value layout".into());
    }
    path.push(ty);
    match ty {
        Type::Record { name, fields } => {
            let mut names = std::collections::BTreeSet::new();
            for field in fields {
                let identifier = !field.name.is_empty()
                    && field.name.bytes().enumerate().all(|(index, byte)| {
                        byte == b'_'
                            || byte.is_ascii_alphabetic()
                            || index > 0 && byte.is_ascii_digit()
                    });
                if !identifier || !names.insert(field.name) {
                    return Err(format!(
                        "native record '{name}' has an invalid or duplicate field"
                    ));
                }
                validate_type(field.ty, path, types)?;
            }
        }
        Type::Array { element, .. } | Type::Collection { element, .. } => {
            validate_type(*element, path, types)?
        }
        _ => {}
    }
    path.pop();
    if ty.slots() == 0 || ty.slots() > VALUE_SLOT_LIMIT {
        return Err(format!(
            "native value layout must occupy 1..={VALUE_SLOT_LIMIT} slots"
        ));
    }
    if let Some(name) = ty.name()
        && types
            .insert(name, ty)
            .is_some_and(|previous| previous != ty)
    {
        return Err(format!("native type '{name}' has conflicting definitions"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageParameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagePart {
    Text(String),
    Argument(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTemplate {
    pub parameters: Vec<MessageParameter>,
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone)]
pub enum ValueLayout {
    Scalar(Type),
    Sequence(Vec<Self>),
    Array {
        element: Box<Self>,
        len: u16,
    },
    /// A zero-based tag, selected payload, then zero padding to the widest payload.
    Variants(Vec<Self>),
}
impl ValueLayout {
    pub(crate) fn validate(&self) -> Result<(), String> {
        fn visit(
            layout: &ValueLayout,
            depth: usize,
            types: &mut BTreeMap<&'static str, Type>,
        ) -> Result<(), String> {
            if depth >= CALL_FRAME_LIMIT {
                return Err("excessively nested parameter layout".into());
            }
            match layout {
                ValueLayout::Scalar(ty) => validate_type(*ty, &mut Vec::new(), types),
                ValueLayout::Sequence(fields) | ValueLayout::Variants(fields) => fields
                    .iter()
                    .try_for_each(|field| visit(field, depth + 1, types)),
                ValueLayout::Array { element, .. } => visit(element, depth + 1, types),
            }
        }
        visit(self, 0, &mut BTreeMap::new())
    }

    pub fn slots(&self) -> Option<usize> {
        match self {
            Self::Scalar(ty) => Some(ty.slots()),
            Self::Sequence(fields) => fields
                .iter()
                .try_fold(0usize, |sum, field| sum.checked_add(field.slots()?)),
            Self::Array { element, len } => element.slots()?.checked_mul(usize::from(*len)),
            Self::Variants(variants) => {
                let first = variants.first()?.slots()?;
                variants[1..]
                    .iter()
                    .try_fold(first, |largest, variant| {
                        Some(largest.max(variant.slots()?))
                    })?
                    .checked_add(1)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub entry: u32,
    /// Flattened parameter slots occupy the beginning of the local frame.
    pub parameters: u16,
    /// Retained structural domains for arguments supplied by the host.
    pub parameter_layout: ValueLayout,
    pub locals: u16,
    pub results: u16,
    pub is_task: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub code: Vec<Op>,
    pub functions: Vec<Function>,
    pub natives: Vec<NativeDeclaration>,
    /// Unique strings: equal values share one index throughout a program.
    pub strings: Vec<String>,
    pub texts: Vec<String>,
    /// Entries refer to the same IDs as `texts`; absent IDs are literal messages.
    pub templates: BTreeMap<u32, MessageTemplate>,
    pub locations: BTreeMap<u32, SourceLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    NegI32,
    NegF32,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    AddI32,
    SubI32,
    MulI32,
    DivI32,
    RemI32,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    Eq,
    Ne,
    LtI32,
    LeI32,
    GtI32,
    GeI32,
    AddF32,
    SubF32,
    MulF32,
    DivF32,
    RemF32,
    EqF32,
    NeF32,
    LtF32,
    LeF32,
    GtF32,
    GeF32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    I32ToF32,
    F32ToI32,
    I32ToTicks,
}
