//! Script identities and physical model owners are separate only for aliases.
use super::*;

pub const MODEL_GROUPS: &str = "meshes/groups.json";

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ModelMappings(pub BTreeMap<u32, ModelTarget>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelTarget {
    Model(String),
    Alias(ModelAlias),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAlias {
    pub name: String,
    pub model: String,
}

impl ModelTarget {
    pub fn name(&self) -> &str {
        match self {
            Self::Model(name) => name,
            Self::Alias(alias) => &alias.name,
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Model(name) => name,
            Self::Alias(alias) => &alias.model,
        }
    }
}

impl<'de> Deserialize<'de> for ModelMappings {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let result =
            Self(crate::library::MappingValues::<ModelTarget>::deserialize(deserializer)?.0);
        result.by_name().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}

impl ModelMappings {
    /// Build once per field preparation, without inspecting any model payloads.
    pub fn by_name(&self) -> Result<BTreeMap<&str, (u32, &str)>> {
        let mut names = BTreeMap::new();
        for (&id, target) in &self.0 {
            validate_path(target.name())?;
            validate_path(target.model())?;
            ensure!(
                names.insert(target.name(), (id, target.model())).is_none(),
                "duplicate model name {}",
                target.name()
            );
        }
        Ok(names)
    }

    pub fn model_path(&self, resource: u32) -> Result<String> {
        let target = self.0.get(&resource).context("unmapped actor resource")?;
        validate_path(target.model())?;
        Ok(format!("meshes/{}/model.json", target.model()))
    }
}

impl Actors {
    pub(super) fn expand_models(
        models: Vec<ModelBinding>,
        read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    ) -> Result<Vec<ModelBinding>> {
        let groups: BTreeMap<String, Vec<ModelBinding>> = if models
            .iter()
            .any(|model| matches!(model, ModelBinding::Group { .. }))
        {
            json(read, MODEL_GROUPS)?
        } else {
            BTreeMap::new()
        };
        let mut result = Vec::new();
        for model in models {
            if let ModelBinding::Group { group } = model {
                let members = groups
                    .get(&group)
                    .with_context(|| format!("unknown model group {group}"))?;
                ensure!(
                    members
                        .iter()
                        .all(|m| !matches!(m, ModelBinding::Group { .. })),
                    "model group {group} cannot contain other groups"
                );
                result.extend(members.iter().cloned());
            } else {
                result.push(model);
            }
        }
        Ok(result)
    }
}
