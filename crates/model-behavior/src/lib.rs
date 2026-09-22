//! Generic, instance-local model operations driven by authored SymphoniaScript.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    model_behavior::{ModelBehaviorBinding, Node},
    model_preview::ModelPreview,
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::authored::{NativeDeclaration, Type};
use symphonia_script_compiler::{ScriptKind, SourceResolver};
use symphonia_script_tools::{PreparationCache, PreparedModule};
use symphonia_script_vm::{Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

const NODE: Type = Type::Handle("model::Node");
const INSTRUCTION_BUDGET: u32 = 10_000;

#[repr(u8)]
enum Native {
    Translation,
    Scale,
    Node,
    Elevation,
    Flag,
}
impl Native {
    const fn declaration(self) -> NativeDeclaration {
        let (name, parameters, result): (_, &[Type], _) = match self {
            Self::Translation => (
                "model::set_translation",
                &[Type::F32, Type::F32, Type::F32],
                None,
            ),
            Self::Scale => (
                "model::set_scale",
                &[NODE, Type::F32, Type::F32, Type::F32],
                None,
            ),
            Self::Node => ("model::node", &[Type::String], Some(NODE)),
            Self::Elevation => ("model::elevation", &[], Some(Type::F32)),
            Self::Flag => ("game::story::flag", &[Type::I32], Some(Type::Bool)),
        };
        NativeDeclaration {
            name,
            opcode: self as u8,
            parameters,
            result,
            suspends: false,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct PoseOverrides {
    pub translation: Option<[f32; 3]>,
    pub scales: BTreeMap<Node, [f32; 3]>,
}

struct ModelHost<'a> {
    nodes: &'a [Vec<Node>],
    names: &'a [String],
    elevation: f32,
    flags: &'a dyn Fn(u16) -> bool,
    pose: PoseOverrides,
}

fn vector(arguments: &[i32]) -> [f32; 3] {
    // Registered native signatures guarantee three finite float arguments.
    std::array::from_fn(|i| f32::from_bits(arguments[i] as u32))
}

impl Host for ModelHost<'_> {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(Native::Translation.declaration(), |host, args, _| {
            host.pose.translation = Some(vector(args));
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Scale.declaration(), |host, args, _| {
            let targets = usize::try_from(args[0])
                .ok()
                .and_then(|i| host.nodes.get(i))
                .ok_or("invalid model node handle")?;
            let scale = vector(&args[1..]);
            for &node in targets {
                host.pose.scales.insert(node, scale);
            }
            Ok(NativeResult::Continue(None))
        })
        .register_typed(Native::Node.declaration(), |host, args, _| {
            // The VM validates this string index against the same interned table.
            let index = args[0] as usize;
            if host.nodes[index].is_empty() {
                return Err(format!(
                    "unknown or ambiguous model node {:?}",
                    host.names[index]
                ));
            }
            Ok(NativeResult::Continue(Some(args[0])))
        })
        .register_typed(Native::Elevation.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(
                Some(host.elevation.to_bits() as i32),
            ))
        })
        .register_typed(Native::Flag.declaration(), |host, args, _| {
            let flag = u16::try_from(args[0]).map_err(|_| "invalid story flag")?;
            Ok(NativeResult::Continue(Some(i32::from((host.flags)(flag)))))
        });
}

pub fn native_declarations() -> Vec<NativeDeclaration> {
    ModelHost::AUTHORED_NATIVES.declarations().collect()
}

/// Names remain verbatim except `#` escaping and occurrence suffixes for duplicates.
/// Both cooking and preparation use these keys; ambiguous bare names never resolve.
pub fn node_names(bones: &[String]) -> Vec<String> {
    bones
        .iter()
        .enumerate()
        .map(|(index, bone)| {
            let name = bone.replace('#', "##");
            if bones.iter().filter(|other| *other == bone).count() > 1 {
                let occurrence = bones[..index].iter().filter(|other| *other == bone).count();
                format!("{name}#{occurrence}")
            } else {
                name
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct PreparedBehavior {
    module: Arc<PreparedModule>,
    entry: u32,
    nodes: Vec<Vec<Node>>,
    elevation: f32,
}
impl PreparedBehavior {
    /// Resolve interned node names once; native calls then use indexed handles.
    pub fn prepare(
        cache: &mut PreparationCache,
        sources: &impl SourceResolver,
        binding: &ModelBehaviorBinding,
        preview: &ModelPreview,
    ) -> Result<Self> {
        binding.validate()?;
        preview.validate()?;
        let generation =
            cache.prepare([binding.module.as_str()], sources, &native_declarations())?;
        let module = generation
            .module(&binding.module)
            .context("missing prepared behavior module")?
            .clone();
        ensure!(
            module.kind == ScriptKind::Model,
            "model behavior requires `script model;`"
        );
        ensure!(
            module.assets.is_empty(),
            "model behavior has unsupported assets"
        );
        let authored = module
            .program
            .authored()
            .context("expected an authored model behavior")?;
        ensure!(
            authored.texts.is_empty(),
            "model behavior cannot display messages"
        );
        let name = format!("{}::{}", binding.module, binding.function);
        let function = authored
            .functions
            .iter()
            .find(|f| f.name == name)
            .with_context(|| format!("missing model behavior function {name}"))?;
        ensure!(
            !function.is_task && function.results == 0 && function.parameters == 0,
            "model behavior must be a parameterless synchronous function without a result"
        );
        let primary = &preview.parts[0].scene.bone_names;
        let names = node_names(primary);
        let mut keys = BTreeMap::<&str, Vec<&str>>::new();
        for (bone, key) in primary.iter().zip(&names) {
            keys.entry(bone).or_default().push(key);
        }
        let mut named = BTreeMap::<&str, Vec<Node>>::new();
        for (part, model) in preview
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.attached_to.is_none())
        {
            let mut occurrences = BTreeMap::new();
            for (bone, name) in model.scene.bone_names.iter().enumerate() {
                let occurrence = occurrences.entry(name).or_insert(0);
                if let Some(key) = keys
                    .get(name.as_str())
                    .and_then(|keys| keys.get(*occurrence))
                {
                    named.entry(*key).or_default().push(Node {
                        part: part.try_into()?,
                        bone: bone.try_into()?,
                    });
                }
                *occurrence += 1;
            }
        }
        let nodes = authored
            .strings
            .iter()
            .map(|name| named.remove(name.as_str()).unwrap_or_default())
            .collect();
        let entry = function.entry;
        Ok(Self {
            module,
            entry,
            nodes,
            elevation: preview.elevation,
        })
    }

    /// Fresh output prevents prior conditions leaking between instances or ticks.
    pub fn evaluate(&self, flags: impl Fn(u16) -> bool) -> Result<PoseOverrides> {
        let mut host = ModelHost {
            nodes: &self.nodes,
            names: &self
                .module
                .program
                .authored()
                .context("expected authored model behavior")?
                .strings,
            elevation: self.elevation,
            flags: &flags,
            pose: Default::default(),
        };
        let mut vm = Vm::new(self.module.program.clone(), self.entry)?;
        let outcome = vm
            .run(&mut host, &mut Memory::default(), INSTRUCTION_BUDGET)
            .map_err(|error| {
                let location = self.module.program.authored().and_then(|m| {
                    m.locations
                        .range(..=error.pc)
                        .next_back()
                        .map(|(_, location)| location)
                });
                match location {
                    Some(location) => anyhow::anyhow!(
                        "{}:{}:{}: {error}",
                        location.file,
                        location.line,
                        location.column
                    ),
                    None => anyhow::anyhow!(error),
                }
            })?;
        ensure!(
            matches!(outcome.event, RunEvent::Halted),
            "model behavior unexpectedly suspended"
        );
        Ok(host.pose)
    }

    pub fn module(&self) -> &PreparedModule {
        &self.module
    }
}

#[cfg(test)]
mod tests;
