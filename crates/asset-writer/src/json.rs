//! Indented objects and short, inline arrays keep editable tables readable.
use serde_json::Value;

/// Round-trip through JSON text to retain f32's short decimal spelling.
pub fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, serde_json::Error> {
    to_vec(&serde_json::from_slice(&serde_json::to_vec(value)?)?)
}

pub fn to_vec(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    fn write(value: &Value, depth: usize, out: &mut String) -> Result<(), serde_json::Error> {
        match value {
            Value::Object(fields) if !fields.is_empty() => {
                out.push_str("{\n");
                for (index, (key, value)) in fields.iter().enumerate() {
                    out.push_str(&"  ".repeat(depth + 1));
                    out.push_str(&serde_json::to_string(key)?);
                    out.push_str(": ");
                    write(value, depth + 1, out)?;
                    if index + 1 != fields.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(depth));
                out.push('}');
            }
            Value::Array(values) => {
                let compact = serde_json::to_string(value)?;
                if depth * 2 + compact.len() <= 110 {
                    out.push_str(&compact);
                } else {
                    out.push_str("[\n");
                    let indent = "  ".repeat(depth + 1);
                    let mut column = 0;
                    let mut previous_scalar = false;
                    for (index, value) in values.iter().enumerate() {
                        let compact = serde_json::to_string(value)?;
                        let scalar = !value.is_array() && !value.is_object();
                        if index == 0 {
                            out.push_str(&indent);
                            column = indent.len();
                        } else {
                            out.push(',');
                            if scalar && previous_scalar && column + compact.len() + 2 <= 110 {
                                out.push(' ');
                                column += 2;
                            } else {
                                out.push('\n');
                                out.push_str(&indent);
                                column = indent.len();
                            }
                        }
                        if column + compact.len() <= 110 {
                            out.push_str(&compact);
                        } else {
                            write(value, depth + 1, out)?;
                        }
                        column += compact.len();
                        previous_scalar = scalar;
                    }
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth));
                    out.push(']');
                }
            }
            value => out.push_str(&serde_json::to_string(value)?),
        }
        Ok(())
    }
    let mut output = String::new();
    write(value, 0, &mut output)?;
    output.push('\n');
    Ok(output.into_bytes())
}
