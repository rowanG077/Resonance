//! Editable message values. Native expression bytes are translated only on import.
use crate::{Op, Program, Width, semantics};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error, ser::SerializeMap};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Constant(i32),
    State {
        offset: u16,
        width: Width,
    },
    Calculate {
        operator: u8,
        arguments: Vec<Self>,
    },
    Call {
        function: String,
        arguments: Vec<Self>,
    },
}

impl Expression {
    pub fn constant(&self) -> Option<i32> {
        match self {
            Self::Constant(value) => Some(*value),
            _ => None,
        }
    }

    /// Translate a complete native expression, rejecting unsupported control flow
    /// instead of embedding another bytecode program in the editable document.
    pub fn from_native(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 4096 || !bytes.len().is_multiple_of(2) {
            return Err("invalid message expression length".into());
        }
        let mut source = vec![0, 4, 0, 0, 0, 0, 0, 0];
        source.extend_from_slice(bytes);
        let program = Program::decode(&source).map_err(|error| error.to_string())?;
        let mut stack = Vec::new();
        let mut arguments = Vec::new();
        let mut result = None;
        let mut pc = program.entry();
        loop {
            let (op, next) = program
                .instruction(pc)
                .ok_or("missing expression instruction")?;
            match op {
                Op::Push(value) => stack.push(Self::Constant(value)),
                Op::Load(offset, width) => stack.push(Self::State { offset, width }),
                Op::Calculate(0) => {
                    if result.is_some() {
                        return Err("unused intermediate message expression".into());
                    }
                    result = Some(stack.pop().ok_or("empty message expression")?);
                }
                Op::Calculate(operator) => {
                    let count = usize::from(
                        semantics::calculator(operator)
                            .ok_or("unknown expression operator")?
                            .inputs,
                    );
                    let start = stack
                        .len()
                        .checked_sub(count)
                        .ok_or("missing expression operands")?;
                    let arguments = stack.split_off(start);
                    stack.push(Self::Calculate {
                        operator,
                        arguments,
                    });
                }
                Op::Argument => arguments.push(result.take().ok_or("missing message argument")?),
                Op::Native(opcode) => {
                    static REGISTRY: std::sync::OnceLock<semantics::NativeRegistry> =
                        std::sync::OnceLock::new();
                    let call = REGISTRY
                        .get_or_init(semantics::NativeRegistry::gqseaf)
                        .get(opcode)
                        .ok_or("unknown message call")?;
                    if call.arguments.len() != arguments.len() {
                        return Err(format!("invalid message arguments for {}", call.name));
                    }
                    stack.push(Self::Call {
                        function: call.name.clone(),
                        arguments: std::mem::take(&mut arguments),
                    });
                }
                Op::End if stack.is_empty() && arguments.is_empty() => {
                    return result.ok_or("missing message result".into());
                }
                _ => return Err("unsupported message expression instruction".into()),
            }
            pc = next;
        }
    }
}

impl Serialize for Expression {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Constant(value) = self {
            return serializer.serialize_i32(*value);
        }
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Self::Constant(_) => unreachable!(),
            Self::State { offset, width } => map.serialize_entry(
                match width {
                    Width::S8 => "state_i8",
                    Width::S16 => "state_i16",
                    Width::S32 => "state_i32",
                },
                offset,
            )?,
            Self::Calculate {
                operator,
                arguments,
            } => {
                let name = semantics::calculator_name(*operator)
                    .ok_or_else(|| serde::ser::Error::custom("unknown expression operator"))?;
                map.serialize_entry(name, arguments)?;
            }
            Self::Call {
                function,
                arguments,
            } => {
                #[derive(Serialize)]
                struct Call<'a> {
                    function: &'a str,
                    arguments: &'a [Expression],
                }
                map.serialize_entry(
                    "call",
                    &Call {
                        function,
                        arguments,
                    },
                )?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Expression {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if let Some(number) = value.as_i64() {
            return Ok(Self::Constant(number.try_into().map_err(D::Error::custom)?));
        }
        let Value::Object(map) = value else {
            return Err(D::Error::custom(
                "expected an integer or named message expression",
            ));
        };
        if map.len() != 1 {
            return Err(D::Error::custom("a message expression has one operation"));
        }
        let (name, value) = map.into_iter().next().unwrap();
        let width = match name.as_str() {
            "state_i8" => Some(Width::S8),
            "state_i16" => Some(Width::S16),
            "state_i32" => Some(Width::S32),
            _ => None,
        };
        if let Some(width) = width {
            return Ok(Self::State {
                offset: serde_json::from_value(value).map_err(D::Error::custom)?,
                width,
            });
        }
        if name == "call" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Call {
                function: String,
                arguments: Vec<Expression>,
            }
            let call: Call = serde_json::from_value(value).map_err(D::Error::custom)?;
            if call.function.is_empty() || call.arguments.len() > 64 {
                return Err(D::Error::custom("invalid message call"));
            }
            return Ok(Self::Call {
                function: call.function,
                arguments: call.arguments,
            });
        }
        let spec = semantics::calculator_by_name(&name)
            .filter(|spec| spec.opcode != 0)
            .ok_or_else(|| D::Error::custom(format!("unknown message operation {name}")))?;
        let arguments: Vec<Self> = serde_json::from_value(value).map_err(D::Error::custom)?;
        if arguments.len() != usize::from(spec.inputs) {
            return Err(D::Error::custom(format!(
                "{name} needs {} operands",
                spec.inputs
            )));
        }
        Ok(Self::Calculate {
            operator: spec.opcode,
            arguments,
        })
    }
}
