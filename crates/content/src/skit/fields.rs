//! Field names in authored availability; numeric IDs only after preparation.
use super::*;
use crate::library::Mappings;
use anyhow::Context;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum FieldRef {
    Name(String),
    /// A boundary can refer to a field whose resource is absent (e.g. ID 94).
    Id {
        id: u16,
    },
}
impl From<u16> for FieldRef {
    fn from(id: u16) -> Self {
        Self::Id { id }
    }
}
impl FieldRef {
    pub fn id(&self) -> Result<u16> {
        match self {
            Self::Id { id } => Ok(*id),
            Self::Name(name) => anyhow::bail!("skit field {name} was not prepared"),
        }
    }
    fn name(&mut self, fields: &Mappings) {
        if let Self::Id { id } = self
            && let Ok(name) = fields.target(u32::from(*id))
        {
            *self = Self::Name(name.into());
        }
    }
    fn resolve(&mut self, fields: &BTreeMap<&str, u16>) -> Result<()> {
        if let Self::Name(name) = self {
            *self = (*fields
                .get(name.as_str())
                .with_context(|| format!("unknown skit field {name}"))?)
            .into();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRange {
    pub from: FieldRef,
    pub through: FieldRef,
}
impl From<[u16; 2]> for FieldRange {
    fn from([from, through]: [u16; 2]) -> Self {
        Self {
            from: from.into(),
            through: through.into(),
        }
    }
}

impl SkitCondition {
    fn visit_fields(&mut self, visit: &mut impl FnMut(&mut FieldRef) -> Result<()>) -> Result<()> {
        match self {
            Self::Maps(range) => {
                visit(&mut range.from)?;
                visit(&mut range.through)?;
            }
            Self::All(conditions) | Self::Any(conditions) => {
                for condition in conditions {
                    condition.visit_fields(visit)?;
                }
            }
            Self::Not(condition) => condition.visit_fields(visit)?,
            _ => {}
        }
        Ok(())
    }
}
impl SkitDefinition {
    fn visit_fields(&mut self, visit: &mut impl FnMut(&mut FieldRef) -> Result<()>) -> Result<()> {
        if let SkitLocation::Map(field) = &mut self.location {
            visit(field)?;
        }
        self.condition.visit_fields(visit)
    }
}

pub fn name_fields(skits: &mut [SkitDefinition], fields: &Mappings) -> Result<()> {
    for skit in skits {
        skit.visit_fields(&mut |field| {
            field.name(fields);
            Ok(())
        })?;
    }
    Ok(())
}

pub fn prepare_fields(skits: &mut [SkitDefinition], fields: &Mappings) -> Result<()> {
    let fields = fields
        .iter()
        .map(|(id, name)| Ok((name, u16::try_from(id)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    for skit in skits {
        skit.visit_fields(&mut |field| field.resolve(&fields))?;
    }
    Ok(())
}
