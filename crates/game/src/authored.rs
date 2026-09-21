//! Explicit source-event preparation, followed by activation in the field's
//! existing scheduler. Preparing an event never advances gameplay or changes it.
use anyhow::{Context, Result, ensure};
use resonance_events::EventRuntime;
use std::sync::Arc;
use symphonia_script::authored::{MessagePart, Type};
pub use symphonia_script_compiler::AssetReference;
use symphonia_script_compiler::{ScriptKind, SourceResolver};
use symphonia_script_tools::{PreparationCache, PreparedModule};
mod field;
pub use field::{FieldEvent, FieldScripts};

pub struct Entry<'a> {
    pub module: &'a str,
    /// Unqualified function name in the entry module.
    pub task: &'a str,
    pub arguments: &'a [i32],
}

/// The scene owner resolves every static asset and message before activation.
/// Message validation must check the scene's available glyphs, including text
/// resources that are not reached along the current execution branch.
pub trait Resources {
    fn asset(&mut self, reference: &AssetReference) -> Result<()>;
    fn message(&mut self, text: &str) -> Result<()>;
    /// Validate every possible label/number glyph for a typed substitution.
    fn substitution(&mut self, ty: Type) -> Result<()>;
}

pub struct PreparedEvent {
    module: Arc<PreparedModule>,
    task: String,
    arguments: Vec<i32>,
}

impl PreparedEvent {
    /// Compile outside gameplay, then validate all resource requirements. A
    /// failed edit or unavailable glyph never returns a stale prepared event.
    pub fn prepare(
        cache: &mut PreparationCache,
        sources: &impl SourceResolver,
        entry: Entry<'_>,
        resources: &mut impl Resources,
    ) -> Result<Self> {
        let generation = cache.prepare(
            [entry.module],
            sources,
            &resonance_events::authored::native_declarations(),
        )?;
        let module = generation
            .module(entry.module)
            .context("prepared entry module is missing")?
            .clone();
        ensure!(
            module.kind == ScriptKind::Field,
            "event entry must declare script field"
        );
        let authored = module
            .program
            .authored()
            .context("expected an authored program")?;
        let task = format!("{}::{}", entry.module, entry.task);
        let function = authored
            .functions
            .iter()
            .find(|function| function.name == task)
            .with_context(|| format!("event entry {task} was not found"))?;
        ensure!(
            function.is_task && function.results == 0,
            "event entry must be a task without a return value"
        );
        ensure!(
            usize::from(function.parameters) == entry.arguments.len(),
            "event entry {task} needs {} argument slots, got {}",
            function.parameters,
            entry.arguments.len()
        );
        symphonia_script_vm::Vm::validate_arguments(
            &module.program,
            function.entry,
            entry.arguments,
        )
        .with_context(|| format!("invalid arguments for event entry {task}"))?;
        prepare_resources(&module, resources)?;
        Ok(Self {
            module,
            task,
            arguments: entry.arguments.to_vec(),
        })
    }

    pub fn module(&self) -> &PreparedModule {
        &self.module
    }

    /// Activation performs no source reads, compilation, or asset loading.
    pub fn start(&self, events: &mut EventRuntime) -> Result<i32> {
        events.start_authored(self.module.program.clone(), &self.task, &self.arguments)
    }
}

pub(crate) fn prepare_resources(
    module: &PreparedModule,
    resources: &mut impl Resources,
) -> Result<()> {
    let authored = module
        .program
        .authored()
        .context("expected authored program")?;
    for asset in &module.assets {
        resources.asset(asset)?;
    }
    for (index, text) in authored.texts.iter().enumerate() {
        if !authored.templates.contains_key(&(index as u32)) {
            resources.message(text)?;
        }
    }
    let mut substitutions = Vec::new();
    for template in authored.templates.values() {
        for part in &template.parts {
            if let MessagePart::Text(text) = part {
                resources.message(text)?;
            }
        }
        for parameter in &template.parameters {
            if !substitutions.contains(&parameter.ty) {
                resources.substitution(parameter.ty)?;
                substitutions.push(parameter.ty);
            }
        }
    }
    Ok(())
}
