//! Oracle-only input pins. Current cooked documents use the production readers;
//! original byte identities belong to independent evidence, never to mod files.
use super::random_inputs::Pin;
use anyhow::{Context as ContextExt, Result, ensure};
use resonance_content::{battle_effect, battle_scene, diagnostics::Diagnostics};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
mod tests;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    version: u32,
    pub entry: String,
    files: BTreeMap<String, Pin>,
    source: Pin,
    /// Observer-only original pool selections, indexed by the current prepared template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect_templates: Option<Vec<Vec<usize>>>,
}

impl Evidence {
    pub fn is_declared(&self) -> bool {
        self.version == 1
            && resonance_content::library::validate_path(&self.entry).is_ok()
            && self.files.contains_key(&self.entry)
            && self.source.sha256.len() == 64
            && self
                .source
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
    }
    pub fn parse(value: &serde_json::Value) -> Result<Self> {
        let evidence: Self = serde_json::from_value(value.clone())?;
        ensure!(
            evidence.version == 1,
            "unsupported named-content evidence version"
        );
        resonance_content::library::validate_path(&evidence.entry)?;
        ensure!(
            evidence.files.contains_key(&evidence.entry),
            "unpinned evidence entry {}",
            evidence.entry
        );
        for path in evidence.files.keys() {
            resonance_content::library::validate_path(path)?;
        }
        Ok(evidence)
    }

    pub fn read(&self, path: &str) -> Result<Arc<[u8]>> {
        ensure!(
            self.version == 1 && self.files.contains_key(&self.entry),
            "invalid current content evidence"
        );
        resonance_content::library::validate_path(path)?;
        Ok(self
            .files
            .get(path)
            .with_context(|| format!("unpinned current evidence resource {path}"))?
            .read_bytes()?
            .into())
    }

    pub fn source(&self) -> Result<Vec<u8>> {
        ensure!(
            self.version == 1 && self.files.contains_key(&self.entry),
            "invalid current content evidence"
        );
        self.source.read_bytes()
    }

    pub fn source_digest(&self) -> Result<&str> {
        self.source()?;
        Ok(&self.source.sha256)
    }

    pub fn scene(&self) -> Result<(battle_scene::Scene, battle_effect::SourceBank)> {
        battle_scene::document::read(&self.entry, &Diagnostics::new(true), |path| self.read(path))
    }

    pub fn current_template(&self, original: usize) -> Result<u64> {
        let templates = self
            .effect_templates
            .as_ref()
            .context("missing observed template bindings")?;
        let (index, _) = super::exactly_one(
            templates
                .iter()
                .enumerate()
                .filter(|(_, aliases)| aliases.contains(&original)),
        )
        .context("missing unique current template for observed behavior")?;
        Ok(index as u64)
    }

    pub fn source_template(&self, member: usize) -> Result<usize> {
        self.effect_templates
            .as_ref()
            .and_then(|templates| templates.get(member))
            .and_then(|aliases| aliases.first())
            .copied()
            .context("effect evidence lacks an original template binding")
    }

    pub fn effects(&self) -> Result<battle_effect::SourceBank> {
        battle_effect::document::read(&self.entry, &Diagnostics::new(true), |path| self.read(path))
    }

    pub fn registered_effect(
        &self,
        setup: &serde_json::Value,
    ) -> Result<battle_effect::SourceBank> {
        let observed = Self::parse(&setup["effect_inputs"][&self.entry])?;
        ensure!(
            self.source_digest()? == observed.source_digest()?,
            "effect source differs from independently captured evidence"
        );
        self.includes(&observed)?;
        observed.includes(self)?;
        ensure!(
            self.effect_templates == observed.effect_templates,
            "original effect bindings differ"
        );
        self.effects()
    }

    fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.read(path)?)?)
    }

    pub fn enemy(
        &self,
        id: u8,
    ) -> Result<(
        resonance_content::battle_enemy::Definition,
        resonance_content::model_preview::ModelPreview,
    )> {
        use resonance_content::{
            battle_enemy::document as enemy, battle_model, model_preview::document as previews,
        };
        let mappings: resonance_content::library::Mappings = self.json(enemy::MAPPINGS)?;
        ensure!(
            mappings.resolve(enemy::MAPPINGS, u32::from(id), Some("enemy.json"))? == self.entry,
            "enemy evidence does not match its machine ID"
        );
        let entry: enemy::Entry = self.json(&self.entry)?;
        let artwork = entry
            .decode_artwork()?
            .context("enemy evidence has no artwork")?;
        let rules = entry.rules;
        let mut preview = previews::read(&artwork.model, |path| self.read(path))?;
        previews::prepare(&mut preview, |path| self.read(path))?;
        preview.validate()?;
        battle_model::Rig::from_glb(
            &self.read(&preview.parts[0].scene.mesh)?,
            &artwork.attachment_nodes,
        )?;
        ensure!(
            preview.parts[0]
                .scene
                .clips
                .iter()
                .all(|clip| !rules.null_motions.contains(&clip.resource_slot)),
            "enemy evidence declares a populated motion as empty"
        );
        Ok((rules.into_definition()?, preview))
    }

    pub fn recoil(&self) -> Result<serde_json::Value> {
        ensure!(
            self.entry == resonance_content::battle_recoil::PATH,
            "wrong current recoil entry"
        );
        let mut files = resonance_content::prepared::Files::default();
        files.insert(self.entry.clone(), self.read(&self.entry)?);
        let characters = resonance_content::battle_model::document::PARTY;
        files.insert(characters.into(), self.read(characters)?);
        Ok(serde_json::to_value(
            resonance_content::battle_rules::recoil(&files)?,
        )?)
    }

    pub fn projectiles(&self) -> Result<serde_json::Value> {
        ensure!(
            self.entry == resonance_content::battle_projectile::PATH,
            "wrong current projectile entry"
        );
        let mut files = resonance_content::prepared::Files::default();
        files.insert(self.entry.clone(), self.read(&self.entry)?);
        Ok(serde_json::to_value(
            resonance_content::battle_rules::projectiles(&files)?,
        )?)
    }

    /// A subresource proof must refer to the same captured current documents.
    pub fn includes(&self, child: &Self) -> Result<()> {
        for (path, pin) in &child.files {
            ensure!(
                self.files
                    .get(path)
                    .is_some_and(|parent| parent.sha256 == pin.sha256),
                "evidence snapshots disagree at {path}"
            );
        }
        Ok(())
    }

    /// Range proof is ordinary byte slicing, with no original-format parser.
    pub fn contains_source(&self, child: &Self, range: [usize; 2]) -> Result<()> {
        let source = self.source()?;
        let child = child.source()?;
        ensure!(
            source.get(range[0]..range[1]) == Some(child.as_slice()),
            "original child evidence differs from its parent byte range"
        );
        Ok(())
    }
}

pub(super) fn registered_scene(
    registration: &serde_json::Value,
) -> Result<(battle_scene::Scene, battle_effect::SourceBank)> {
    let scene = Evidence::parse(&registration["scene"])?;
    let effects = Evidence::parse(&registration["effects"])?;
    ensure!(
        registration["package_sha256"] == scene.source_digest()?
            && registration["bank_sha256"] == effects.source_digest()?,
        "registered scene original byte identities differ"
    );
    scene.includes(&effects)?;
    scene.contains_source(
        &effects,
        serde_json::from_value(registration["effects_range"].clone())?,
    )?;
    let (document, bank) = scene.scene()?;
    ensure!(
        document.effects == effects.entry,
        "registered effects do not belong to the current scene"
    );
    Ok((document, bank))
}

pub(super) fn effect_value(bank: battle_effect::SourceBank) -> Result<serde_json::Value> {
    let art = bank
        .art
        .map(|art| serde_json::json!({"textures": art.textures, "models": art.models}));
    Ok(serde_json::json!({"programs": bank.programs, "actors": bank.actors, "art": art}))
}

pub(super) fn action_catalogue(trace: &super::Trace) -> Result<serde_json::Value> {
    let pin: Pin = serde_json::from_value(trace.setup["catalogue"].clone())?;
    let bytes = pin.read_bytes()?;
    if trace.case.starts_with("enemy:") {
        let entry: resonance_content::battle_enemy::document::Entry =
            serde_json::from_slice(&bytes)?;
        Ok(serde_json::to_value(entry.rules.into_definition()?)?)
    } else {
        Ok(serde_json::from_slice(&bytes)?)
    }
}

/// Native constants identify observed behaviors; the editable bank uses local names.
/// Resolve that relationship from the external observation receipt only.
pub(super) fn template_member(trace: &super::Trace, bank: &str, original: usize) -> Result<u64> {
    let receipts = trace.setup["effect_inputs"]
        .as_object()
        .context("missing effect observations")?;
    let receipt = super::exactly_one(
        receipts
            .values()
            .filter(|value| value["source"]["sha256"] == bank),
    )
    .context("missing unique effect observation")?;
    let templates = receipt["effect_templates"]
        .as_array()
        .context("missing observed template bindings")?;
    let index = super::exactly_one(templates.iter().enumerate().filter(|(_, aliases)| {
        aliases
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id.as_u64() == Some(original as u64)))
    }))
    .context("missing unique current template for observed behavior")?
    .0;
    Ok(index as u64)
}
