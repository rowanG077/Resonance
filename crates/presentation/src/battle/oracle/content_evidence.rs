//! Oracle-only input pins. Current cooked documents use the production readers;
//! original byte identities belong to independent evidence, never to mod files.
use super::random_inputs::Pin;
use anyhow::{Context, Result, ensure};
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
        self.effects()
    }

    fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.read(path)?)?)
    }

    pub fn enemy(
        &self,
        id: u8,
    ) -> Result<(
        resonance_content::battle_enemy::document::Rules,
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
        let rules: enemy::Rules = self.json(&entry.rules)?;
        let artwork: battle_model::document::enemy::Definition = self.json(&entry.artwork)?;
        ensure!(
            entry.version == enemy::VERSION
                && rules.version == enemy::VERSION
                && artwork.version == battle_model::document::VERSION,
            "unsupported enemy evidence document"
        );
        let rig: battle_model::Rig = self.json(&artwork.rig)?;
        rig.skeleton.validate()?;
        let mut preview = previews::read(&artwork.model, |path| self.read(path))?;
        previews::prepare(&mut preview, |path| self.read(path))?;
        preview.validate()?;
        ensure!(
            rig.skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(preview.parts[0].scene.bone_names.iter()),
            "enemy evidence rig differs from its model"
        );
        ensure!(
            preview.parts[0]
                .scene
                .clips
                .iter()
                .all(|clip| !rules.null_motions.contains(&clip.resource_slot)),
            "enemy evidence declares a populated motion as empty"
        );
        Ok((rules, preview))
    }

    pub fn recoil(&self) -> Result<serde_json::Value> {
        ensure!(
            self.entry == resonance_content::battle_recoil::PATH,
            "wrong current recoil entry"
        );
        let mut files = resonance_content::prepared::Files::default();
        files.insert(self.entry.clone(), self.read(&self.entry)?);
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
        let rules: resonance_content::battle_enemy::document::Rules =
            serde_json::from_slice(&bytes)?;
        ensure!(
            rules.version == resonance_content::battle_enemy::document::VERSION,
            "unsupported enemy action catalogue"
        );
        Ok(serde_json::to_value(rules)?)
    } else {
        Ok(serde_json::from_slice(&bytes)?)
    }
}
