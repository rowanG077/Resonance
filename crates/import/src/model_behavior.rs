//! Bind authored behavior to source catalogue records without evaluating it.
use crate::all_assets::figurine_catalogue::Resource;
use anyhow::{Result, ensure};
use resonance_content::model_behavior::ModelBehaviorBinding;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Catalogue {
    Figurines,
    Monsters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subject {
    Figurine(Resource),
    Monster(u8),
}
impl Subject {
    fn catalogue(self) -> Catalogue {
        match self {
            Self::Figurine(_) => Catalogue::Figurines,
            Self::Monster(_) => Catalogue::Monsters,
        }
    }
}

#[derive(Deserialize)]
enum Figurine {
    Efreet,
}
impl Figurine {
    fn resource(self) -> Resource {
        match self {
            Self::Efreet => Resource::TaggedNpc(73),
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[repr(u8)]
enum Monster {
    SwordDancer = 191,
    TheFugitive = 208,
    TheNeglected = 209,
    TheJudged = 210,
    Yggdrasill236 = 236,
    Yggdrasill237 = 237,
    Yggdrasill238 = 238,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Selector {
    Figurines(Vec<Figurine>),
    Monsters(Vec<Monster>),
}
impl Selector {
    fn subjects(self) -> Vec<Subject> {
        match self {
            Self::Figurines(ids) => ids
                .into_iter()
                .map(|id| Subject::Figurine(id.resource()))
                .collect(),
            Self::Monsters(ids) => ids
                .into_iter()
                .map(|id| Subject::Monster(id as u8))
                .collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    select: Selector,
    module: String,
    function: String,
}

struct Binding {
    subject: Subject,
    entry: ModelBehaviorBinding,
    seen: bool,
}
pub(crate) struct Bindings(Vec<Binding>);

impl Bindings {
    pub(crate) fn new() -> Result<Self> {
        Self::parse(resonance_script_content::PREVIEW_BINDINGS)
    }

    fn parse(manifest: &str) -> Result<Self> {
        let rules: Vec<Rule> = serde_json::from_str(manifest)?;
        let mut bindings: Vec<Binding> = Vec::new();
        for rule in rules {
            let subjects = rule.select.subjects();
            ensure!(!subjects.is_empty(), "empty preview behavior selector");
            let entry = ModelBehaviorBinding {
                module: rule.module,
                function: rule.function,
            };
            entry.validate()?;
            for subject in subjects {
                ensure!(
                    !bindings.iter().any(|binding| binding.subject == subject),
                    "duplicate preview behavior selector {subject:?}"
                );
                bindings.push(Binding {
                    subject,
                    entry: entry.clone(),
                    seen: false,
                });
            }
        }
        Ok(Self(bindings))
    }

    pub(crate) fn finish(&self, catalogue: Catalogue) -> Result<()> {
        for binding in self
            .0
            .iter()
            .filter(|binding| binding.subject.catalogue() == catalogue)
        {
            ensure!(
                binding.seen,
                "preview behavior selector has no catalogue record: {:?}",
                binding.subject
            );
        }
        Ok(())
    }

    pub(crate) fn bind(&mut self, subject: Subject) -> Option<ModelBehaviorBinding> {
        let binding = self
            .0
            .iter_mut()
            .find(|binding| binding.subject == subject)?;
        binding.seen = true;
        Some(binding.entry.clone())
    }
}

#[cfg(test)]
pub(crate) mod tests;

pub(crate) fn publish(output: &Path) -> Result<()> {
    for &(relative, source) in resonance_script_content::FILES {
        let path = format!("scripts/{relative}");
        resonance_content::validate_asset_path(&path)?;
        crate::write_atomic(&output.join(path), source.as_bytes())?;
    }
    Ok(())
}
