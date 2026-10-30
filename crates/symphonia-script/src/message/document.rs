//! Ordinary strings, with named inline controls only where they are needed.
use super::{Expression, Message, Token};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error, ser::SerializeMap};
use serde_json::Value;
use symphonia_constants::CharacterId;

fn control(opcode: u8) -> Option<&'static str> {
    Some(match opcode {
        1 => "actor_name",
        2 => "delay",
        3 => "color",
        4 => "item_name",
        5 => "number",
        6 => "evaluate",
        7 => "unidentified_control_7",
        8 => "number_input",
        9 => "voice",
        17 => "title_name",
        18 => "technique_name",
        _ => return None,
    })
}

const COLORS: [&str; 8] = [
    "white", "blue", "red", "purple", "green", "cyan", "yellow", "flashing",
];

fn label(opcode: u8, value: i32) -> Option<&'static str> {
    match opcode {
        1 if value == 999_999 => Some("controlled"),
        1 => u8::try_from(value)
            .ok()
            .and_then(|id| CharacterId::try_from(id).ok())
            .and_then(CharacterId::resource_name),
        3 => usize::try_from(value)
            .ok()
            .and_then(|i| COLORS.get(i))
            .copied(),
        9 if value == -1 => Some("stop"),
        _ => None,
    }
}

impl Serialize for Token {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Text { text } = self {
            return serializer.serialize_str(text);
        }
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Self::Text { .. } => unreachable!(),
            Self::Raw { value } => map.serialize_entry("unresolved_byte", value)?,
            Self::Control { opcode, expression } => {
                let name = control(*opcode)
                    .ok_or_else(|| serde::ser::Error::custom("unknown message control"))?;
                if let Some(label) = expression
                    .constant()
                    .and_then(|value| label(*opcode, value))
                {
                    map.serialize_entry(name, label)?;
                } else {
                    map.serialize_entry(name, expression)?;
                }
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if let Value::String(text) = value {
            return Ok(Self::Text { text });
        }
        let Value::Object(map) = value else {
            return Err(D::Error::custom("expected text or a named message control"));
        };
        if map.len() != 1 {
            return Err(D::Error::custom("a message control has one name"));
        }
        let (name, value) = map.into_iter().next().unwrap();
        if name == "unresolved_byte" {
            return Ok(Self::Raw {
                value: serde_json::from_value(value).map_err(D::Error::custom)?,
            });
        }
        let opcode = [1, 2, 3, 4, 5, 6, 7, 8, 9, 17, 18]
            .into_iter()
            .find(|op| control(*op) == Some(&name))
            .ok_or_else(|| D::Error::custom(format!("unknown message control {name}")))?;
        let expression = if let Value::String(name) = value {
            let value = match opcode {
                1 if name == "controlled" => Some(999_999),
                1 => CharacterId::ALL
                    .iter()
                    .find(|id| id.resource_name() == Some(name.as_str()))
                    .map(|id| i32::from(id.raw())),
                3 => COLORS
                    .iter()
                    .position(|color| *color == name)
                    .map(|i| i as i32),
                9 if name == "stop" => Some(-1),
                _ => None,
            }
            .ok_or_else(|| D::Error::custom(format!("unknown {name} value for message control")))?;
            Expression::Constant(value)
        } else {
            serde_json::from_value(value).map_err(D::Error::custom)?
        };
        Ok(Self::Control { opcode, expression })
    }
}

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self
            .tokens
            .iter()
            .all(|token| matches!(token, Token::Text { .. }))
        {
            let text: String = self
                .tokens
                .iter()
                .filter_map(|token| match token {
                    Token::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            serializer.serialize_str(&text)
        } else {
            self.tokens.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Document {
            Text(String),
            Parts(Vec<Token>),
        }
        Ok(Self {
            tokens: match Document::deserialize(deserializer)? {
                Document::Text(text) if text.is_empty() => vec![],
                Document::Text(text) => vec![Token::Text { text }],
                Document::Parts(tokens) => tokens,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dialogue_is_text_with_named_controls_and_editable_values() {
        let document = json!([
            "Hello.", "", [{"actor_name":"lloyd"}, ", wait!", {"color":"red"},
                {"voice":1179693}, {"number":{"add":[{"state_i32":2064},1]}}, {"voice":"stop"}]
        ]);
        let messages: Vec<Message> = serde_json::from_value(document.clone()).unwrap();
        assert_eq!(serde_json::to_value(&messages).unwrap(), document);
        assert!(messages[1].tokens.is_empty());
        assert_eq!(
            messages[2].tokens[0],
            Token::Control {
                opcode: 1,
                expression: Expression::Constant(1)
            }
        );
        assert_eq!(
            messages[2].tokens[2],
            Token::Control {
                opcode: 3,
                expression: Expression::Constant(2)
            }
        );
        for invalid in [
            json!({"tokens":[]}),
            json!([{"opcode":1,"expression":[0,1,48,0,32,255]}]),
            json!([{"actor_name":"invented"}]),
            json!([{"color":"red","voice":3}]),
            json!([{"number":{"add":[1]}}]),
            json!([{"number":{"mystery":[1,2]}}]),
        ] {
            assert!(serde_json::from_value::<Message>(invalid).is_err());
        }
        let minimum = Message {
            tokens: vec![Token::Control {
                opcode: 1,
                expression: Expression::Constant(i32::MIN),
            }],
        };
        assert!(serde_json::to_value(minimum).is_ok());
    }
}
