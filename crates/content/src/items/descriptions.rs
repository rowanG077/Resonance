use super::*;

/// Most descriptions are plain strings; only a description that suppresses
/// another needs an object. Text stays independently diagnosable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Description {
    Suppressing(SuppressingDescription),
    Text(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuppressingDescription {
    pub text: Value,
    pub suppresses: Vec<String>,
}

impl Description {
    pub fn new(text: String, suppresses: Vec<String>) -> Self {
        if suppresses.is_empty() {
            Self::Text(text.into())
        } else {
            Self::Suppressing(SuppressingDescription {
                text: text.into(),
                suppresses,
            })
        }
    }

    pub(super) fn parts(&self) -> Result<(&[String], &Value)> {
        match self {
            Self::Suppressing(rule) => Ok((&rule.suppresses, &rule.text)),
            Self::Text(text) => {
                ensure!(
                    !text.is_object(),
                    "equipment description object needs text and suppresses"
                );
                Ok((&[], text))
            }
        }
    }
}

impl Prepared {
    pub fn description_text(&self, diagnostics: &Diagnostics) -> Result<BTreeMap<String, String>> {
        let mut text = BTreeMap::new();
        for (name, value) in &self.description_text {
            if let Some(value) = diagnostics.attempt(
                &format!("equipment description {name}"),
                serde_json::from_value(value.clone()).map_err(Into::into),
            )? {
                text.insert(name.clone(), value);
            }
        }
        Ok(text)
    }
}
