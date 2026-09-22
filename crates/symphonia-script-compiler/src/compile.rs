use crate::{
    AssetReference, Compilation, Diagnostic, Location, ScriptKind, SourceResolver,
    syntax::{self, Expr, Expression as E, Pattern, Statement, StatementKind as S, TypeRef},
};
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script::{
    Op, Program,
    authored::{
        self, BinaryOp as B, Conversion, MAX_MESSAGE_ARGUMENTS, MessageParameter, MessagePart,
        MessageTemplate, NativeDeclaration, Type as Scalar, UnaryOp as U, ValueLayout,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ty {
    Unit,
    Scalar(Scalar),
    Array(Box<Ty>, u16),
    Record(String),
    Enum(String),
    Option(Box<Ty>),
    Task(Box<Ty>),
}
impl Ty {
    fn scalar(&self) -> Option<Scalar> {
        if let Self::Scalar(ty) = self {
            Some(*ty)
        } else {
            None
        }
    }
}
const INT: Ty = Ty::Scalar(Scalar::I32);
const BOOL: Ty = Ty::Scalar(Scalar::Bool);
const FLOAT: Ty = Ty::Scalar(Scalar::F32);

fn native_ty(ty: Scalar) -> Ty {
    match ty {
        Scalar::Record { name, .. } => Ty::Record(name.into()),
        Scalar::Array { element, len } => Ty::Array(Box::new(native_ty(*element)), len),
        _ => Ty::Scalar(ty),
    }
}

#[derive(Clone)]
struct Record {
    fields: Vec<(String, Ty)>,
}
#[derive(Clone)]
struct Function {
    module: String,
    ast: syntax::Function,
    parameters: Vec<Ty>,
    result: Ty,
    index: u16,
}
#[derive(Clone)]
struct Constant {
    ty: Ty,
    words: Vec<i32>,
}
#[derive(Clone)]
struct Message {
    index: u32,
    parameters: Vec<Ty>,
    public: bool,
}

struct Compiler {
    modules: BTreeMap<String, syntax::Module>,
    natives: Vec<NativeDeclaration>,
    native_types: BTreeMap<String, Scalar>,
    records: BTreeMap<String, Record>,
    enums: BTreeMap<String, Vec<(String, Vec<Ty>)>>,
    constants: BTreeMap<String, Constant>,
    resolving_constants: BTreeSet<String>,
    functions: BTreeMap<String, Function>,
    code: Vec<Op>,
    texts: Vec<String>,
    strings: Vec<String>,
    templates: BTreeMap<u32, MessageTemplate>,
    messages: BTreeMap<String, Message>,
    assets: Vec<AssetReference>,
    locations: BTreeMap<u32, authored::SourceLocation>,
    calls: BTreeMap<u16, BTreeSet<u16>>,
}

pub(super) fn compile(
    root: &str,
    sources: &impl SourceResolver,
    natives: &[NativeDeclaration],
) -> Result<Compilation, Diagnostic> {
    let at = Location {
        file: root.into(),
        line: 1,
        column: 1,
    };
    authored::validate_natives(natives).map_err(|message| at.error(message))?;
    let mut modules = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    load(
        root,
        sources,
        natives,
        &mut modules,
        &mut inputs,
        &mut BTreeSet::new(),
        &at,
    )?;
    let kind = modules[root].kind;
    if kind != ScriptKind::Library && modules[root].functions.is_empty() {
        return Err(at.error("field and model scripts must declare an entry function or task"));
    }
    let mut compiler = Compiler {
        modules,
        natives: natives.to_vec(),
        native_types: BTreeMap::new(),
        records: BTreeMap::new(),
        enums: BTreeMap::new(),
        constants: BTreeMap::new(),
        resolving_constants: BTreeSet::new(),
        functions: BTreeMap::new(),
        code: Vec::new(),
        texts: Vec::new(),
        strings: Vec::new(),
        templates: BTreeMap::new(),
        messages: BTreeMap::new(),
        assets: Vec::new(),
        locations: BTreeMap::new(),
        calls: BTreeMap::new(),
    };
    for native in natives {
        for ty in native.parameters.iter().copied().chain(native.result) {
            compiler.native_type(ty);
        }
    }
    compiler.declarations(root)?;
    let mut ordered: Vec<_> = compiler.functions.values().cloned().collect();
    ordered.sort_by_key(|f| f.index);
    let mut functions = Vec::new();
    for function in ordered {
        let entry = compiler.code.len() as u32;
        let parameters = function.parameters.iter().try_fold(0u16, |total, ty| {
            total
                .checked_add(compiler.width(ty, &function.ast.at)?)
                .ok_or_else(|| function.ast.at.error("too many parameter slots"))
        })?;
        let results = compiler.width(&function.result, &function.ast.at)?;
        let parameter_layout = ValueLayout::Sequence(
            function
                .parameters
                .iter()
                .map(|ty| compiler.layout(ty))
                .collect(),
        );
        let mut body = Body {
            compiler: &mut compiler,
            function: &function,
            scopes: vec![Scope::default()],
            next_local: 0,
            loops: Vec::new(),
            in_cleanup: false,
        };
        for ((name, _), ty) in function.ast.parameters.iter().zip(&function.parameters) {
            body.local(name, ty.clone(), false, &function.ast.at)?;
        }
        body.block(&function.ast.body)?;
        if function.result != Ty::Unit && !returns(&function.ast.body) {
            return Err(function
                .ast
                .at
                .error("function must return a value on every path"));
        }
        body.emit(Op::ReturnValues(0), &function.ast.at);
        let locals = body.next_local;
        functions.push(authored::Function {
            name: format!("{}::{}", function.module, function.ast.name),
            entry,
            parameters,
            parameter_layout,
            locals,
            results,
            is_task: function.ast.task,
        });
    }
    for function in &functions {
        let index = compiler.functions[&function.name].index;
        if reaches(index, index, &compiler.calls, &mut BTreeSet::new()) {
            return Err(at.error(format!(
                "recursive function cycle contains '{}'",
                function.name
            )));
        }
    }
    let program = Program::from_authored(authored::Module {
        code: compiler.code,
        functions,
        natives: compiler.natives,
        texts: compiler.texts,
        strings: compiler.strings,
        templates: compiler.templates,
        locations: compiler.locations,
    })
    .map_err(|error| at.error(error.to_string()))?;
    Ok(Compilation {
        kind,
        program,
        assets: compiler.assets,
        sources: inputs,
    })
}

fn load(
    name: &str,
    sources: &impl SourceResolver,
    natives: &[NativeDeclaration],
    modules: &mut BTreeMap<String, syntax::Module>,
    inputs: &mut BTreeMap<String, String>,
    visiting: &mut BTreeSet<String>,
    at: &Location,
) -> Result<(), Diagnostic> {
    if modules.contains_key(name) {
        return Ok(());
    }
    if !visiting.insert(name.into()) {
        return Err(at.error(format!("import cycle through '{name}'")));
    }
    let source = sources
        .source(name)
        .ok_or_else(|| at.error(format!("module '{name}' was not found")))?;
    let module = syntax::parse(name, source)?;
    for (import, location) in &module.imports {
        let source_module = if sources.source(import).is_some() {
            Some(import.as_str())
        } else {
            import
                .rsplit_once("::")
                .map(|(parent, _)| parent)
                .filter(|parent| sources.source(parent).is_some())
        };
        if let Some(imported) = source_module {
            load(
                imported, sources, natives, modules, inputs, visiting, location,
            )?;
            check_import(module.kind, modules[imported].kind, imported, location)?;
        } else if !native_import(natives, import) {
            return Err(location.error(format!("import '{import}' was not found")));
        }
    }
    visiting.remove(name);
    inputs.insert(name.into(), source.into());
    modules.insert(name.into(), module);
    Ok(())
}

fn check_import(
    kind: ScriptKind,
    imported: ScriptKind,
    name: &str,
    at: &Location,
) -> Result<(), Diagnostic> {
    if kind == imported || imported == ScriptKind::Library {
        Ok(())
    } else {
        Err(at.error(format!(
            "{kind} script cannot use {imported} module '{name}'"
        )))
    }
}

fn native_import(natives: &[NativeDeclaration], import: &str) -> bool {
    let prefix = format!("{import}::");
    fn contains(ty: Scalar, import: &str, prefix: &str) -> bool {
        ty.name()
            .is_some_and(|name| name == import || name.starts_with(prefix))
            || match ty {
                Scalar::Record { fields, .. } => fields
                    .iter()
                    .any(|field| contains(field.ty, import, prefix)),
                Scalar::Array { element, .. } | Scalar::Collection { element, .. } => {
                    contains(*element, import, prefix)
                }
                _ => false,
            }
    }
    natives.iter().any(|native| {
        native.name == import
            || native.name.starts_with(&prefix)
            || native
                .parameters
                .iter()
                .copied()
                .chain(native.result)
                .any(|ty| contains(ty, import, &prefix))
    })
}

impl Compiler {
    fn native_type(&mut self, ty: Scalar) {
        if let Some(name) = ty.name() {
            self.native_types.insert(name.into(), ty);
        }
        match ty {
            Scalar::Record { name, fields } => {
                for field in fields {
                    self.native_type(field.ty);
                }
                self.records.insert(
                    name.into(),
                    Record {
                        fields: fields
                            .iter()
                            .map(|field| (field.name.into(), native_ty(field.ty)))
                            .collect(),
                    },
                );
            }
            Scalar::Array { element, .. } | Scalar::Collection { element, .. } => {
                self.native_type(*element)
            }
            _ => {}
        }
    }
    fn candidates(&self, module: &str, name: &str) -> Vec<String> {
        let mut candidates = vec![format!("{module}::{name}")];
        for (import, _) in &self.modules[module].imports {
            let short = import.rsplit("::").next().unwrap();
            if name == short {
                candidates.push(import.clone());
            }
            if let Some(rest) = name.strip_prefix(&format!("{short}::")) {
                candidates.push(format!("{import}::{rest}"));
            }
        }
        candidates.push(name.into());
        candidates
    }
    fn visible(
        &self,
        module: &str,
        qualified: &str,
        public: bool,
        at: &Location,
    ) -> Result<(), Diagnostic> {
        if let Some((owner, _)) = qualified.rsplit_once("::")
            && let Some(imported) = self.modules.get(owner)
        {
            check_import(self.modules[module].kind, imported.kind, owner, at)?;
        }
        if qualified
            .rsplit_once("::")
            .is_some_and(|(parent, _)| parent == module)
            || public
        {
            Ok(())
        } else {
            Err(at.error(format!("'{qualified}' is private")))
        }
    }
    fn ty(&self, module: &str, source: &TypeRef, at: &Location) -> Result<Ty, Diagnostic> {
        Ok(match source {
            TypeRef::Array(element, length) => {
                let element = self.ty(module, element, at)?;
                value_type(&element, at)?;
                Ty::Array(Box::new(element), *length)
            }
            TypeRef::Optional(element) => {
                let element = self.ty(module, element, at)?;
                value_type(&element, at)?;
                Ty::Option(Box::new(element))
            }
            TypeRef::Task(result) => {
                let result = result
                    .as_ref()
                    .map(|result| self.ty(module, result, at))
                    .transpose()?
                    .unwrap_or(Ty::Unit);
                value_type(&result, at)?;
                Ty::Task(Box::new(result))
            }
            TypeRef::Named(name) => match name.as_str() {
                "i32" => INT,
                "f32" => FLOAT,
                "bool" => BOOL,
                "Ticks" | "ticks" => Ty::Scalar(Scalar::Ticks),
                "Message" => Ty::Scalar(Scalar::Message),
                "string" => Ty::Scalar(Scalar::String),
                _ => {
                    let found = self
                        .candidates(module, name)
                        .into_iter()
                        .find_map(|qualified| {
                            if let Some(ty) = self.native_types.get(&qualified) {
                                return Some(Ok(native_ty(*ty)));
                            }
                            for (owner, module_ast) in &self.modules {
                                if let Some(record) = module_ast
                                    .records
                                    .iter()
                                    .find(|v| format!("{owner}::{}", v.name) == qualified)
                                {
                                    return Some(
                                        self.visible(module, &qualified, record.public, at)
                                            .map(|()| Ty::Record(qualified)),
                                    );
                                }
                                if let Some(enumeration) = module_ast
                                    .enums
                                    .iter()
                                    .find(|v| format!("{owner}::{}", v.name) == qualified)
                                {
                                    return Some(
                                        self.visible(module, &qualified, enumeration.public, at)
                                            .map(|()| Ty::Enum(qualified)),
                                    );
                                }
                            }
                            None
                        });
                    found.ok_or_else(|| at.error(format!("unknown type '{name}'")))??
                }
            },
        })
    }
    fn width(&self, ty: &Ty, at: &Location) -> Result<u16, Diagnostic> {
        fn width(compiler: &Compiler, ty: &Ty, visiting: &mut BTreeSet<String>) -> Option<u16> {
            match ty {
                Ty::Unit => Some(0),
                Ty::Scalar(ty) => Some(ty.slots() as u16),
                Ty::Task(_) => Some(1),
                Ty::Array(item, length) => width(compiler, item, visiting)?.checked_mul(*length),
                Ty::Option(item) => width(compiler, item, visiting)?.checked_add(1),
                Ty::Record(name) => {
                    if !visiting.insert(name.clone()) {
                        return None;
                    }
                    let result = compiler
                        .records
                        .get(name)?
                        .fields
                        .iter()
                        .try_fold(0u16, |sum, (_, ty)| {
                            sum.checked_add(width(compiler, ty, visiting)?)
                        });
                    visiting.remove(name);
                    result
                }
                Ty::Enum(name) => {
                    if !visiting.insert(name.clone()) {
                        return None;
                    }
                    let mut largest = 0;
                    for (_, parameters) in compiler.enums.get(name)? {
                        let size = parameters.iter().try_fold(0u16, |sum, ty| {
                            sum.checked_add(width(compiler, ty, visiting)?)
                        })?;
                        largest = largest.max(size);
                    }
                    visiting.remove(name);
                    largest.checked_add(1)
                }
            }
        }
        width(self, ty, &mut BTreeSet::new())
            .ok_or_else(|| at.error("recursive or oversized value layout"))
    }
    fn layout(&self, ty: &Ty) -> ValueLayout {
        match ty {
            Ty::Unit => ValueLayout::Sequence(Vec::new()),
            Ty::Scalar(ty) => ValueLayout::Scalar(*ty),
            Ty::Array(element, len) => ValueLayout::Array {
                element: Box::new(self.layout(element)),
                len: *len,
            },
            Ty::Record(name) => ValueLayout::Sequence(
                self.records[name]
                    .fields
                    .iter()
                    .map(|(_, ty)| self.layout(ty))
                    .collect(),
            ),
            Ty::Enum(name) => ValueLayout::Variants(
                self.enums[name]
                    .iter()
                    .map(|(_, types)| {
                        ValueLayout::Sequence(types.iter().map(|ty| self.layout(ty)).collect())
                    })
                    .collect(),
            ),
            Ty::Option(element) => ValueLayout::Variants(vec![
                ValueLayout::Sequence(Vec::new()),
                self.layout(element),
            ]),
            Ty::Task(_) => unreachable!("task handles cannot cross function parameters"),
        }
    }
    fn declarations(&mut self, root: &str) -> Result<(), Diagnostic> {
        let modules = self.modules.clone();
        for (module, ast) in &modules {
            let mut names = BTreeSet::new();
            for (name, at) in ast
                .records
                .iter()
                .map(|v| (&v.name, &v.at))
                .chain(ast.enums.iter().map(|v| (&v.name, &v.at)))
                .chain(ast.bindings.iter().map(|v| (&v.name, &v.value.at)))
                .chain(ast.functions.iter().map(|v| (&v.name, &v.at)))
                .chain(ast.messages.iter().map(|v| (&v.name, &v.at)))
            {
                if !names.insert(name)
                    || self.native_types.contains_key(&format!("{module}::{name}"))
                {
                    return Err(at.error(format!("duplicate declaration '{name}'")));
                }
            }
            let mut aliases = BTreeSet::new();
            for (import, at) in &ast.imports {
                let alias = import.rsplit("::").next().unwrap();
                if names.contains(&alias.to_owned()) || !aliases.insert(alias) {
                    return Err(at.error(format!("duplicate imported name '{alias}'")));
                }
                if !modules.contains_key(import) && !native_import(&self.natives, import) {
                    let public = import.rsplit_once("::").and_then(|(parent, name)| {
                        modules.get(parent).and_then(|module| {
                            module
                                .functions
                                .iter()
                                .find(|v| v.name == name)
                                .map(|v| v.public)
                                .or_else(|| {
                                    module
                                        .records
                                        .iter()
                                        .find(|v| v.name == name)
                                        .map(|v| v.public)
                                })
                                .or_else(|| {
                                    module
                                        .enums
                                        .iter()
                                        .find(|v| v.name == name)
                                        .map(|v| v.public)
                                })
                                .or_else(|| {
                                    module
                                        .bindings
                                        .iter()
                                        .find(|v| v.name == name)
                                        .map(|v| v.public)
                                })
                                .or_else(|| {
                                    module
                                        .messages
                                        .iter()
                                        .find(|v| v.name == name)
                                        .map(|v| v.public)
                                })
                        })
                    });
                    match public {
                        Some(true) => {}
                        Some(false) => {
                            return Err(at.error(format!("import '{import}' is private")));
                        }
                        None => return Err(at.error(format!("import '{import}' was not found"))),
                    }
                }
            }
            for record in &ast.records {
                let mut fields = Vec::new();
                for (name, ty) in &record.fields {
                    if fields.iter().any(|(field, _)| field == name) {
                        return Err(record.at.error(format!("duplicate field '{name}'")));
                    }
                    let ty = self.ty(module, ty, &record.at)?;
                    value_type(&ty, &record.at)?;
                    fields.push((name.clone(), ty));
                }
                self.records
                    .insert(format!("{module}::{}", record.name), Record { fields });
            }
            for enumeration in &ast.enums {
                let variants: BTreeSet<_> =
                    enumeration.variants.iter().map(|(name, _)| name).collect();
                if variants.len() != enumeration.variants.len() || variants.is_empty() {
                    return Err(enumeration
                        .at
                        .error("enum variants must be unique and nonempty"));
                }
                let variants = enumeration
                    .variants
                    .iter()
                    .map(|(name, parameters)| {
                        Ok((
                            name.clone(),
                            parameters
                                .iter()
                                .map(|ty| {
                                    let ty = self.ty(module, ty, &enumeration.at)?;
                                    value_type(&ty, &enumeration.at)?;
                                    Ok(ty)
                                })
                                .collect::<Result<_, Diagnostic>>()?,
                        ))
                    })
                    .collect::<Result<_, Diagnostic>>()?;
                self.enums
                    .insert(format!("{module}::{}", enumeration.name), variants);
            }
        }
        for (module, ast) in &modules {
            for message in &ast.messages {
                if message.parameters.len() > MAX_MESSAGE_ARGUMENTS {
                    return Err(message.at.error(format!(
                        "message accepts at most {MAX_MESSAGE_ARGUMENTS} arguments"
                    )));
                }
                let mut names = BTreeSet::new();
                let mut parameters = Vec::new();
                let mut types = Vec::new();
                for (name, reference) in &message.parameters {
                    if !names.insert(name) {
                        return Err(message
                            .at
                            .error(format!("duplicate message parameter '{name}'")));
                    }
                    let ty = self.ty(module, reference, &message.at)?;
                    let Ty::Scalar(scalar @ (Scalar::I32 | Scalar::TextReference { .. })) = ty
                    else {
                        return Err(message
                            .at
                            .error("message substitutions require i32 or a typed text reference"));
                    };
                    parameters.push(MessageParameter {
                        name: name.clone(),
                        ty: scalar,
                    });
                    types.push(Ty::Scalar(scalar));
                }
                let parts = message_parts(&message.text, &parameters, &message.at)?;
                let index = self.texts.len() as u32;
                self.texts.push(message.text.clone());
                self.templates
                    .insert(index, MessageTemplate { parameters, parts });
                self.messages.insert(
                    format!("{module}::{}", message.name),
                    Message {
                        index,
                        parameters: types,
                        public: message.public,
                    },
                );
            }
            for record in &ast.records {
                self.width(
                    &Ty::Record(format!("{module}::{}", record.name)),
                    &record.at,
                )?;
            }
            for enumeration in &ast.enums {
                self.width(
                    &Ty::Enum(format!("{module}::{}", enumeration.name)),
                    &enumeration.at,
                )?;
            }
        }
        let mut order: Vec<_> = modules.iter().collect();
        order.sort_by_key(|(name, _)| (*name != root, *name));
        for (module, ast) in order {
            for function in &ast.functions {
                let index = u16::try_from(self.functions.len())
                    .map_err(|_| function.at.error("too many functions"))?;
                let parameters = function
                    .parameters
                    .iter()
                    .map(|(_, ty)| {
                        let ty = self.ty(module, ty, &function.at)?;
                        value_type(&ty, &function.at)?;
                        Ok(ty)
                    })
                    .collect::<Result<_, Diagnostic>>()?;
                let result = function
                    .result
                    .as_ref()
                    .map(|ty| self.ty(module, ty, &function.at))
                    .transpose()?
                    .unwrap_or(Ty::Unit);
                value_type(&result, &function.at)?;
                self.functions.insert(
                    format!("{module}::{}", function.name),
                    Function {
                        module: module.clone(),
                        ast: function.clone(),
                        parameters,
                        result,
                        index,
                    },
                );
            }
        }
        for (module, ast) in &modules {
            for binding in &ast.bindings {
                self.constant(&format!("{module}::{}", binding.name), &binding.value.at)?;
            }
        }
        Ok(())
    }
    fn text(&mut self, value: &str) -> i32 {
        if let Some(index) = self.texts.iter().enumerate().position(|(index, text)| {
            text == value && !self.templates.contains_key(&(index as u32))
        }) {
            return index as i32;
        }
        let index = self.texts.len();
        self.texts.push(value.into());
        index as i32
    }
    fn string(&mut self, value: &str) -> i32 {
        if let Some(index) = self.strings.iter().position(|text| text == value) {
            index as i32
        } else {
            let index = self.strings.len();
            self.strings.push(value.into());
            index as i32
        }
    }
    fn constant(&mut self, name: &str, at: &Location) -> Result<Constant, Diagnostic> {
        if let Some(value) = self.constants.get(name) {
            return Ok(value.clone());
        }
        if !self.resolving_constants.insert(name.into()) {
            return Err(at.error(format!("constant cycle through '{name}'")));
        }
        let (module, short) = name.rsplit_once("::").unwrap();
        let binding = self.modules[module]
            .bindings
            .iter()
            .find(|binding| binding.name == short)
            .unwrap()
            .clone();
        let declared = binding
            .ty
            .as_ref()
            .map(|ty| self.ty(module, ty, at))
            .transpose()?;
        let value = if binding.asset {
            let Some(Ty::Scalar(Scalar::Asset(kind))) = declared else {
                return Err(at.error("asset declaration needs a native asset type"));
            };
            let E::Text(path) = &binding.value.kind else {
                return Err(at.error("asset declaration needs a literal logical resource path"));
            };
            if path.is_empty()
                || path.starts_with('/')
                || path.split('/').any(|part| part == ".." || part.is_empty())
            {
                return Err(at.error("asset path must be a nonempty logical resource path"));
            }
            let index = self
                .assets
                .iter()
                .position(|asset| asset.kind == kind && asset.path == *path)
                .unwrap_or_else(|| {
                    let index = self.assets.len();
                    self.assets.push(AssetReference {
                        index: index as u32,
                        kind: kind.into(),
                        path: path.clone(),
                    });
                    index
                });
            Constant {
                ty: Ty::Scalar(Scalar::Asset(kind)),
                words: vec![index as i32],
            }
        } else {
            self.constant_expr(module, &binding.value, declared.as_ref())?
        };
        if let Some(declared) = declared {
            same(&declared, &value.ty, at)?;
        }
        self.resolving_constants.remove(name);
        self.constants.insert(name.into(), value.clone());
        Ok(value)
    }
    fn constant_expr(
        &mut self,
        module: &str,
        expr: &Expr,
        expected: Option<&Ty>,
    ) -> Result<Constant, Diagnostic> {
        let (ty, words) = match &expr.kind {
            E::Number(number) => {
                let (ty, value) = number_value(number, &expr.at)?;
                (ty, vec![value])
            }
            E::Bool(value) => (BOOL, vec![i32::from(*value)]),
            E::Text(value) if expected == Some(&Ty::Scalar(Scalar::Message)) => {
                let mut words = vec![0; Scalar::Message.slots()];
                words[0] = self.text(value);
                (Ty::Scalar(Scalar::Message), words)
            }
            E::Text(value) => (Ty::Scalar(Scalar::String), vec![self.string(value)]),
            E::Name(name) => return self.named_constant(module, name, &expr.at),
            E::Unary(op, value) if op == "-" => {
                if let E::Number(number) = &value.kind {
                    let (ty, value) = number_value(&format!("-{number}"), &expr.at)?;
                    (ty, vec![value])
                } else {
                    return Err(expr
                        .at
                        .error("constant negation requires a numeric literal"));
                }
            }
            E::Array(items) => {
                let hint = match expected {
                    Some(Ty::Array(item, _)) => Some(item.as_ref()),
                    _ => None,
                };
                let mut ty = hint.cloned();
                let mut words = Vec::new();
                for item in items {
                    let value = self.constant_expr(module, item, ty.as_ref())?;
                    if let Some(ty) = &ty {
                        same(ty, &value.ty, &item.at)?;
                    } else {
                        ty = Some(value.ty.clone());
                    }
                    words.extend(value.words);
                }
                (
                    Ty::Array(
                        Box::new(ty.ok_or_else(|| {
                            expr.at.error("empty array requires an explicit type")
                        })?),
                        array_length(items.len(), &expr.at)?,
                    ),
                    words,
                )
            }
            _ => {
                return Err(expr
                    .at
                    .error("constant requires a literal, array or another constant"));
            }
        };
        if let Some(expected) = expected {
            same(expected, &ty, &expr.at)?;
        }
        Ok(Constant { ty, words })
    }
    fn named_constant(
        &mut self,
        module: &str,
        name: &str,
        at: &Location,
    ) -> Result<Constant, Diagnostic> {
        for qualified in self.candidates(module, name) {
            if let Some((owner, short)) = qualified.rsplit_once("::") {
                if let Some(binding) = self
                    .modules
                    .get(owner)
                    .and_then(|ast| ast.bindings.iter().find(|binding| binding.name == short))
                {
                    self.visible(module, &qualified, binding.public, at)?;
                    return self.constant(&qualified, at);
                }
                if let Some(variants) = self.enums.get(owner)
                    && let Some(index) = variants.iter().position(|(variant, _)| variant == short)
                {
                    if !variants[index].1.is_empty() {
                        return Err(at.error("enum variant requires payload arguments"));
                    }
                    let ty = self.ty(module, &TypeRef::Named(owner.into()), at)?;
                    let mut words = vec![0; self.width(&ty, at)? as usize];
                    words[0] = index as i32;
                    return Ok(Constant { ty, words });
                }
            }
        }
        Err(at.error(format!("unknown value '{name}'")))
    }
}

#[derive(Clone)]
struct Local {
    base: u16,
    ty: Ty,
    mutable: bool,
}
struct Loop {
    continues: Vec<usize>,
    breaks: Vec<usize>,
    scope_depth: usize,
}
#[derive(Clone)]
struct Deferred {
    body: Vec<Statement>,
    bindings: Vec<BTreeMap<String, Local>>,
}
#[derive(Default)]
struct Scope {
    locals: BTreeMap<String, Local>,
    defers: Vec<Deferred>,
}
enum Iteration {
    Range,
    Array { base: u16, length: u16, item: Ty },
    Host { handle: u16, item: Scalar, get: u8 },
}
struct Body<'a> {
    compiler: &'a mut Compiler,
    function: &'a Function,
    scopes: Vec<Scope>,
    next_local: u16,
    loops: Vec<Loop>,
    in_cleanup: bool,
}
impl Body<'_> {
    fn emit(&mut self, op: Op, at: &Location) -> usize {
        let index = self.compiler.code.len();
        self.compiler.code.push(op);
        self.compiler.locations.insert(
            index as u32,
            authored::SourceLocation {
                file: at.file.clone(),
                line: at.line,
                column: at.column,
            },
        );
        index
    }
    fn patch(&mut self, instruction: usize, target: u32) {
        self.compiler.code[instruction] = match self.compiler.code[instruction] {
            Op::Jump(_) => Op::Jump(target),
            Op::BranchFalseStack(_) => Op::BranchFalseStack(target),
            _ => unreachable!(),
        };
    }
    fn pc(&self) -> u32 {
        self.compiler.code.len() as u32
    }
    fn reserve(&mut self, ty: &Ty, at: &Location) -> Result<u16, Diagnostic> {
        let base = self.next_local;
        self.next_local = base
            .checked_add(self.compiler.width(ty, at)?)
            .ok_or_else(|| at.error("too many local slots"))?;
        Ok(base)
    }
    fn local(
        &mut self,
        name: &str,
        ty: Ty,
        mutable: bool,
        at: &Location,
    ) -> Result<Local, Diagnostic> {
        if self.scopes.last().unwrap().locals.contains_key(name) {
            return Err(at.error(format!("duplicate local '{name}'")));
        }
        let local = Local {
            base: self.reserve(&ty, at)?,
            ty,
            mutable,
        };
        self.scopes
            .last_mut()
            .unwrap()
            .locals
            .insert(name.into(), local.clone());
        Ok(local)
    }
    fn find_local(&self, name: &str) -> Option<Local> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.locals.get(name).cloned())
    }
    fn store(&mut self, base: u16, ty: &Ty, at: &Location) -> Result<(), Diagnostic> {
        for offset in (0..self.compiler.width(ty, at)?).rev() {
            self.emit(Op::StoreLocal(base + offset), at);
        }
        Ok(())
    }
    fn load(&mut self, base: u16, ty: &Ty, at: &Location) -> Result<(), Diagnostic> {
        for offset in 0..self.compiler.width(ty, at)? {
            self.emit(Op::LoadLocal(base + offset), at);
        }
        Ok(())
    }
    fn block(&mut self, statements: &[Statement]) -> Result<(), Diagnostic> {
        self.scopes.push(Scope::default());
        for statement in statements {
            self.statement(statement)?;
        }
        self.unwind(self.scopes.len() - 1)?;
        self.scopes.pop();
        Ok(())
    }
    fn unwind(&mut self, first: usize) -> Result<(), Diagnostic> {
        let defers: Vec<_> = self.scopes[first..]
            .iter()
            .rev()
            .flat_map(|scope| scope.defers.iter().rev().cloned())
            .collect();
        for deferred in defers {
            let bindings = deferred
                .bindings
                .into_iter()
                .map(|locals| Scope {
                    locals,
                    defers: Vec::new(),
                })
                .collect();
            let scopes = std::mem::replace(&mut self.scopes, bindings);
            let loops = std::mem::take(&mut self.loops);
            let in_cleanup = std::mem::replace(&mut self.in_cleanup, true);
            let result = self.block(&deferred.body);
            self.scopes = scopes;
            self.loops = loops;
            self.in_cleanup = in_cleanup;
            result?;
        }
        Ok(())
    }
    fn statement(&mut self, statement: &Statement) -> Result<(), Diagnostic> {
        let at = &statement.at;
        match &statement.kind {
            S::Block(body) => self.block(body)?,
            S::Defer(body) => {
                let bindings = self
                    .scopes
                    .iter()
                    .map(|scope| scope.locals.clone())
                    .collect();
                self.scopes.last_mut().unwrap().defers.push(Deferred {
                    body: body.clone(),
                    bindings,
                });
            }
            S::Let {
                name,
                mutable,
                ty,
                value,
            } => {
                let hint = ty
                    .as_ref()
                    .map(|ty| self.compiler.ty(&self.function.module, ty, at))
                    .transpose()?;
                let ty = self.expr(value, hint.as_ref())?;
                if ty == Ty::Unit {
                    return Err(at.error("cannot bind a function with no result"));
                }
                let local = self.local(name, ty, *mutable, at)?;
                self.store(local.base, &local.ty, at)?;
            }
            S::Assign(target, op, value) => {
                let (local, index) = self.place(target)?;
                if !local.mutable {
                    return Err(at.error("assignment requires a mutable local"));
                }
                let temporary_index = if let Some(index) = index {
                    let slot = self.reserve(&INT, at)?;
                    self.emit(Op::StoreLocal(slot), at);
                    Some((slot, index))
                } else {
                    None
                };
                if op != "=" {
                    if self.compiler.width(&local.ty, at)? != 1 {
                        return Err(at.error("compound assignment requires a scalar"));
                    }
                    self.read_place(&local, temporary_index, at)?;
                }
                let ty = self.expr(value, Some(&local.ty))?;
                if op != "=" {
                    self.binary(&op[..1], &ty, at)?;
                }
                if let Some((index_slot, length)) = temporary_index {
                    let temp = self.reserve(&ty, at)?;
                    self.store(temp, &ty, at)?;
                    for offset in 0..self.compiler.width(&ty, at)? {
                        self.emit(Op::LoadLocal(index_slot), at);
                        self.emit(Op::LoadLocal(temp + offset), at);
                        self.emit(
                            Op::StoreLocalIndexed {
                                base: local.base + offset,
                                len: length,
                            },
                            at,
                        );
                    }
                } else {
                    self.store(local.base, &local.ty, at)?;
                }
            }
            S::Expression(expr) => {
                let ty = self.expr(expr, None)?;
                for _ in 0..self.compiler.width(&ty, at)? {
                    self.emit(Op::Pop, at);
                }
            }
            S::Return(value) => {
                if self.in_cleanup {
                    return Err(at.error("cleanup cannot return from its enclosing function"));
                }
                let expected = self.function.result.clone();
                let ty = if let Some(value) = value {
                    self.expr(value, Some(&expected))?
                } else {
                    Ty::Unit
                };
                same(&expected, &ty, at)?;
                self.unwind(0)?;
                self.emit(Op::ReturnValues(self.compiler.width(&ty, at)?), at);
            }
            S::If(test, yes, no) => {
                self.expr(test, Some(&BOOL))?;
                let branch = self.emit(Op::BranchFalseStack(0), at);
                self.block(yes)?;
                let jump = self.emit(Op::Jump(0), at);
                self.patch(branch, self.pc());
                self.block(no)?;
                self.patch(jump, self.pc());
            }
            S::While(test, body) => {
                let start = self.pc();
                self.expr(test, Some(&BOOL))?;
                let exit = self.emit(Op::BranchFalseStack(0), at);
                self.loops.push(Loop {
                    continues: Vec::new(),
                    breaks: vec![exit],
                    scope_depth: self.scopes.len(),
                });
                self.block(body)?;
                self.emit(Op::Jump(start), at);
                self.finish_loop(start);
            }
            S::For {
                name,
                start,
                end,
                body,
            } => self.for_loop(name, start, end.as_ref(), body, at)?,
            S::Match(value, arms) => self.match_value(value, arms, at)?,
            S::Break | S::Continue => {
                if self.loops.is_empty() {
                    return Err(at.error(if self.in_cleanup {
                        "cleanup cannot exit an outer loop"
                    } else {
                        "loop control used outside a loop"
                    }));
                }
                self.unwind(self.loops.last().unwrap().scope_depth)?;
                let jump = self.emit(Op::Jump(0), at);
                let cycle = self.loops.last_mut().unwrap();
                if matches!(statement.kind, S::Break) {
                    cycle.breaks.push(jump);
                } else {
                    cycle.continues.push(jump);
                }
            }
        }
        Ok(())
    }
    fn match_value(
        &mut self,
        value: &Expr,
        arms: &[(Pattern, Vec<Statement>)],
        at: &Location,
    ) -> Result<(), Diagnostic> {
        let ty = self.expr(value, None)?;
        let temporary = self.reserve(&ty, at)?;
        self.store(temporary, &ty, at)?;
        let variants = match &ty {
            Ty::Enum(name) => Some(self.compiler.enums[name].clone()),
            Ty::Option(inner) => Some(vec![
                ("None".into(), vec![]),
                ("Some".into(), vec![inner.as_ref().clone()]),
            ]),
            &BOOL => Some(vec![("false".into(), vec![]), ("true".into(), vec![])]),
            &INT => None,
            _ => return Err(at.error("match expects an enum, optional value, bool or i32")),
        };
        let mut covered = BTreeSet::new();
        let mut wildcard = false;
        let mut endings = Vec::new();
        for (pattern, body) in arms {
            if wildcard {
                return Err(at.error("match arm after wildcard is unreachable"));
            }
            self.scopes.push(Scope::default());
            let (tag, bindings) = match pattern {
                Pattern::Wildcard => {
                    wildcard = true;
                    (None, Vec::new())
                }
                Pattern::Bool(value) if ty == BOOL => (Some(i32::from(*value)), Vec::new()),
                Pattern::Integer(value) if ty == INT => {
                    let (literal_ty, value) = number_value(value, at)?;
                    same(&INT, &literal_ty, at)?;
                    (Some(value), Vec::new())
                }
                Pattern::Variant(name, bindings) => {
                    let variant_name = name.rsplit("::").next().unwrap();
                    if matches!(ty, Ty::Option(_))
                        && name
                            .rsplit_once("::")
                            .is_some_and(|(prefix, _)| prefix != "Option")
                    {
                        return Err(at.error("optional pattern must use Some or None"));
                    }
                    if matches!(&ty, Ty::Enum(_)) {
                        let Some((prefix, _)) = name.rsplit_once("::") else {
                            return Err(at.error("enum pattern must include its enum type"));
                        };
                        same(
                            &ty,
                            &self.compiler.ty(
                                &self.function.module,
                                &TypeRef::Named(prefix.into()),
                                at,
                            )?,
                            at,
                        )?;
                    }
                    let variants = variants
                        .as_ref()
                        .ok_or_else(|| at.error("variant pattern requires an enum"))?;
                    let (tag, (_, payload)) = variants
                        .iter()
                        .enumerate()
                        .find(|(_, (variant, _))| variant == variant_name)
                        .ok_or_else(|| at.error(format!("unknown variant '{name}'")))?;
                    if payload.len() != bindings.len() {
                        return Err(at.error("variant pattern has the wrong number of bindings"));
                    }
                    let bindings: Vec<(String, Ty)> = bindings
                        .iter()
                        .cloned()
                        .zip(payload.iter().cloned())
                        .collect();
                    (Some(tag as i32), bindings)
                }
                _ => return Err(at.error("pattern does not match the value type")),
            };
            let branch = if let Some(tag) = tag {
                if !covered.insert(tag) {
                    return Err(at.error("duplicate match arm"));
                }
                self.emit(Op::LoadLocal(temporary), at);
                self.emit(Op::Push(tag), at);
                self.emit(Op::Binary(B::Eq), at);
                Some(self.emit(Op::BranchFalseStack(0), at))
            } else {
                None
            };
            let mut offset = 1;
            for (name, payload) in bindings {
                if name != "_" {
                    let local = self.local(&name, payload.clone(), false, at)?;
                    self.load(temporary + offset, &payload, at)?;
                    self.store(local.base, &payload, at)?;
                }
                offset += self.compiler.width(&payload, at)?;
            }
            self.block(body)?;
            endings.push(self.emit(Op::Jump(0), at));
            if let Some(branch) = branch {
                self.patch(branch, self.pc());
            }
            self.scopes.pop();
        }
        if !wildcard
            && variants
                .as_ref()
                .is_none_or(|variants| covered.len() != variants.len())
        {
            return Err(at.error("match is not exhaustive"));
        }
        for jump in endings {
            self.patch(jump, self.pc());
        }
        Ok(())
    }
    fn finish_loop(&mut self, continue_pc: u32) {
        let cycle = self.loops.pop().unwrap();
        for jump in cycle.continues {
            self.patch(jump, continue_pc);
        }
        for jump in cycle.breaks {
            self.patch(jump, self.pc());
        }
    }
    fn for_loop(
        &mut self,
        name: &str,
        start: &Expr,
        end: Option<&Expr>,
        body: &[Statement],
        at: &Location,
    ) -> Result<(), Diagnostic> {
        self.scopes.push(Scope::default());
        let index = self.reserve(&INT, at)?;
        let iteration =
            if let Some(end) = end {
                self.expr(start, Some(&INT))?;
                self.emit(Op::StoreLocal(index), at);
                self.expr(end, Some(&INT))?;
                Iteration::Range
            } else {
                let ty = self.expr(start, None)?;
                let base = self.reserve(&ty, at)?;
                self.store(base, &ty, at)?;
                self.emit(Op::Push(0), at);
                self.emit(Op::StoreLocal(index), at);
                match ty {
                    Ty::Array(item, length) => {
                        self.emit(Op::Push(i32::from(length)), at);
                        Iteration::Array {
                            base,
                            length,
                            item: *item,
                        }
                    }
                    Ty::Scalar(Scalar::Collection {
                        element,
                        count,
                        get,
                        ..
                    }) => {
                        self.emit(Op::LoadLocal(base), at);
                        self.emit(Op::ArgumentValue, at);
                        self.emit(Op::Native(count), at);
                        Iteration::Host {
                            handle: base,
                            item: *element,
                            get,
                        }
                    }
                    _ => return Err(at.error(
                        "for expects an integer range, fixed array or read-only host collection",
                    )),
                }
            };
        let limit = self.reserve(&INT, at)?;
        self.emit(Op::StoreLocal(limit), at);
        let item_ty = match &iteration {
            Iteration::Range => INT,
            Iteration::Array { item, .. } => item.clone(),
            Iteration::Host { item, .. } => Ty::Scalar(*item),
        };
        let item = self.local(name, item_ty, false, at)?;
        let begin = self.pc();
        self.emit(Op::LoadLocal(index), at);
        self.emit(Op::LoadLocal(limit), at);
        self.emit(Op::Binary(B::LtI32), at);
        let exit = self.emit(Op::BranchFalseStack(0), at);
        match iteration {
            Iteration::Array { base, length, item } => {
                let stride = self.compiler.width(&item, at)?;
                for offset in 0..stride {
                    self.emit(Op::LoadLocal(index), at);
                    self.emit(Op::Push(i32::from(stride)), at);
                    self.emit(Op::Binary(B::MulI32), at);
                    self.emit(
                        Op::LoadLocalIndexed {
                            base: base + offset,
                            len: length * stride,
                        },
                        at,
                    );
                }
            }
            Iteration::Host { handle, get, .. } => {
                self.emit(Op::LoadLocal(handle), at);
                self.emit(Op::ArgumentValue, at);
                self.emit(Op::LoadLocal(index), at);
                self.emit(Op::ArgumentValue, at);
                self.emit(Op::Native(get), at);
            }
            Iteration::Range => {
                self.emit(Op::LoadLocal(index), at);
            }
        }
        self.store(item.base, &item.ty, at)?;
        self.loops.push(Loop {
            continues: Vec::new(),
            breaks: vec![exit],
            scope_depth: self.scopes.len(),
        });
        self.block(body)?;
        let advance = self.pc();
        self.emit(Op::LoadLocal(index), at);
        self.emit(Op::Push(1), at);
        self.emit(Op::Binary(B::AddI32), at);
        self.emit(Op::StoreLocal(index), at);
        self.emit(Op::Jump(begin), at);
        self.finish_loop(advance);
        self.scopes.pop();
        Ok(())
    }
    /// For indexed places, leaves the scalar-buffer index on the value stack.
    fn place(&mut self, expr: &Expr) -> Result<(Local, Option<u16>), Diagnostic> {
        match &expr.kind {
            E::Name(name) if self.find_local(name).is_some() => {
                Ok((self.find_local(name).unwrap(), None))
            }
            E::Field(value, field) => {
                let (mut local, index) = self.place(value)?;
                let Ty::Record(name) = &local.ty else {
                    return Err(expr.at.error("field access requires a record"));
                };
                let record = self.compiler.records[name].clone();
                let mut offset = 0;
                for (name, ty) in record.fields {
                    if name == *field {
                        local.base += offset;
                        local.ty = ty;
                        return Ok((local, index));
                    }
                    offset += self.compiler.width(&ty, &expr.at)?;
                }
                Err(expr.at.error(format!("unknown field '{field}'")))
            }
            E::Index(value, index) => {
                let (mut local, prior) = self.place(value)?;
                if prior.is_some() {
                    return Err(expr
                        .at
                        .error("nested dynamic indexing requires an intermediate local"));
                }
                let Ty::Array(item, length) = &local.ty else {
                    return Err(expr.at.error("indexing requires a fixed array"));
                };
                let item = item.as_ref().clone();
                let length = *length;
                self.expr(index, Some(&INT))?;
                let stride = self.compiler.width(&item, &expr.at)?;
                self.emit(Op::Push(i32::from(stride)), &expr.at);
                self.emit(Op::Binary(B::MulI32), &expr.at);
                local.ty = item;
                Ok((local, Some(length * stride)))
            }
            _ => {
                let ty = self.expr(expr, None)?;
                let base = self.reserve(&ty, &expr.at)?;
                self.store(base, &ty, &expr.at)?;
                Ok((
                    Local {
                        base,
                        ty,
                        mutable: false,
                    },
                    None,
                ))
            }
        }
    }
    fn read_place(
        &mut self,
        local: &Local,
        index: Option<(u16, u16)>,
        at: &Location,
    ) -> Result<(), Diagnostic> {
        if let Some((slot, length)) = index {
            for offset in 0..self.compiler.width(&local.ty, at)? {
                self.emit(Op::LoadLocal(slot), at);
                self.emit(
                    Op::LoadLocalIndexed {
                        base: local.base + offset,
                        len: length,
                    },
                    at,
                );
            }
        } else {
            self.load(local.base, &local.ty, at)?;
        }
        Ok(())
    }
    fn expr(&mut self, expr: &Expr, expected: Option<&Ty>) -> Result<Ty, Diagnostic> {
        let at = &expr.at;
        let ty = match &expr.kind {
            E::Number(number) => {
                let (ty, value) = number_value(number, at)?;
                self.emit(Op::Push(value), at);
                ty
            }
            E::Bool(value) => {
                self.emit(Op::Push(i32::from(*value)), at);
                BOOL
            }
            E::Text(value) if expected == Some(&Ty::Scalar(Scalar::Message)) => {
                let index = self.compiler.text(value);
                self.emit(Op::Push(index), at);
                for _ in 0..MAX_MESSAGE_ARGUMENTS {
                    self.emit(Op::Push(0), at);
                }
                Ty::Scalar(Scalar::Message)
            }
            E::Text(value) => {
                let index = self.compiler.string(value);
                self.emit(Op::Push(index), at);
                Ty::Scalar(Scalar::String)
            }
            E::Name(name) if name == "None" => {
                let Some(Ty::Option(inner)) = expected else {
                    return Err(at.error("None requires an Option type"));
                };
                for _ in 0..=self.compiler.width(inner, at)? {
                    self.emit(Op::Push(0), at);
                }
                expected.unwrap().clone()
            }
            E::Name(name) => {
                if let Some(local) = self.find_local(name) {
                    self.load(local.base, &local.ty, at)?;
                    local.ty
                } else {
                    let value = self
                        .compiler
                        .named_constant(&self.function.module, name, at)?;
                    for word in value.words {
                        self.emit(Op::Push(word), at);
                    }
                    value.ty
                }
            }
            E::Field(_, _) | E::Index(_, _) => {
                let (local, index) = self.place(expr)?;
                let index = if let Some(length) = index {
                    let slot = self.reserve(&INT, at)?;
                    self.emit(Op::StoreLocal(slot), at);
                    Some((slot, length))
                } else {
                    None
                };
                self.read_place(&local, index, at)?;
                local.ty
            }
            E::Unary(op, value) => {
                if op == "-"
                    && let E::Number(number) = &value.kind
                {
                    let (ty, value) = number_value(&format!("-{number}"), at)?;
                    self.emit(Op::Push(value), at);
                    if let Some(expected) = expected {
                        same(expected, &ty, at)?;
                    }
                    return Ok(ty);
                }
                let ty = self.expr(value, None)?;
                let operation = match (op.as_str(), &ty) {
                    ("-", &INT) => U::NegI32,
                    ("-", &FLOAT) => U::NegF32,
                    ("!", &BOOL) => U::Not,
                    ("!", &INT) => U::BitNot,
                    _ => return Err(at.error("invalid unary operation for this type")),
                };
                self.emit(Op::Unary(operation), at);
                ty
            }
            E::Binary(op, left, right) if op == "&&" || op == "||" => {
                self.expr(left, Some(&BOOL))?;
                if op == "||" {
                    self.emit(Op::Unary(U::Not), at);
                }
                let branch = self.emit(Op::BranchFalseStack(0), at);
                self.expr(right, Some(&BOOL))?;
                let done = self.emit(Op::Jump(0), at);
                self.patch(branch, self.pc());
                self.emit(Op::Push(i32::from(op == "||")), at);
                self.patch(done, self.pc());
                BOOL
            }
            E::Binary(op, left, right) => {
                let ty = self.expr(left, None)?;
                self.expr(right, Some(&ty))?;
                self.binary(op, &ty, at)?
            }
            E::Call(name, arguments) => self.call(name, arguments, false, expected, at)?,
            E::Await(value) => {
                if let E::Call(name, arguments) = &value.kind {
                    self.call(name, arguments, true, expected, at)?
                } else {
                    self.check_await(true, true, at)?;
                    let Ty::Task(result) = self.expr(value, None)? else {
                        return Err(at.error("await expects a task handle or suspendable call"));
                    };
                    self.emit(
                        Op::JoinTask {
                            results: self.compiler.width(&result, at)?,
                        },
                        at,
                    );
                    *result
                }
            }
            E::Spawn(name, arguments) => self.spawn(name, arguments, at)?,
            E::Array(items) => {
                let hint = match expected {
                    Some(Ty::Array(item, _)) => Some(item.as_ref()),
                    _ => None,
                };
                let mut item_ty = hint.cloned();
                for item in items {
                    let ty = self.expr(item, item_ty.as_ref())?;
                    if ty == Ty::Unit {
                        return Err(item.at.error("array element cannot be unit"));
                    }
                    value_type(&ty, &item.at)?;
                    item_ty = Some(ty);
                }
                Ty::Array(
                    Box::new(
                        item_ty.ok_or_else(|| at.error("empty array needs an explicit type"))?,
                    ),
                    array_length(items.len(), at)?,
                )
            }
            E::Record(name, fields) => {
                let ty =
                    self.compiler
                        .ty(&self.function.module, &TypeRef::Named(name.clone()), at)?;
                let Ty::Record(record_name) = &ty else {
                    return Err(at.error("record literal requires a record type"));
                };
                let record = self.compiler.records[record_name].clone();
                if fields.len() != record.fields.len() {
                    return Err(at.error("record literal must provide every field exactly once"));
                }
                let mut seen = BTreeSet::new();
                let temporary = self.reserve(&ty, at)?;
                // Preserve authored evaluation order while laying out declaration order.
                for (name, value) in fields {
                    if !seen.insert(name) {
                        return Err(at.error(format!("duplicate field '{name}'")));
                    }
                    let mut offset = 0;
                    let mut found = false;
                    for (field, field_ty) in &record.fields {
                        if name == field {
                            self.expr(value, Some(field_ty))?;
                            self.store(temporary + offset, field_ty, at)?;
                            found = true;
                            break;
                        }
                        offset += self.compiler.width(field_ty, at)?;
                    }
                    if !found {
                        return Err(at.error(format!("unknown field '{name}'")));
                    }
                }
                self.load(temporary, &ty, at)?;
                ty
            }
        };
        if let Some(expected) = expected {
            same(expected, &ty, at)?;
        }
        Ok(ty)
    }
    fn binary(&mut self, op: &str, ty: &Ty, at: &Location) -> Result<Ty, Diagnostic> {
        let float = ty == &FLOAT;
        let numeric = matches!(ty, &INT | &FLOAT | Ty::Scalar(Scalar::Ticks));
        let comparison = matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=");
        let equality = matches!(op, "==" | "!=");
        if (!numeric && !equality) || self.compiler.width(ty, at)? != 1 {
            return Err(at.error("operator is not defined for this type"));
        }
        let operation = match (op, float) {
            ("+", false) => B::AddI32,
            ("-", false) => B::SubI32,
            ("*", false) => B::MulI32,
            ("/", false) => B::DivI32,
            ("%", false) => B::RemI32,
            ("+", true) => B::AddF32,
            ("-", true) => B::SubF32,
            ("*", true) => B::MulF32,
            ("/", true) => B::DivF32,
            ("%", true) => B::RemF32,
            ("==", false) => B::Eq,
            ("!=", false) => B::Ne,
            ("==", true) => B::EqF32,
            ("!=", true) => B::NeF32,
            ("<", false) => B::LtI32,
            ("<=", false) => B::LeI32,
            (">", false) => B::GtI32,
            (">=", false) => B::GeI32,
            ("<", true) => B::LtF32,
            ("<=", true) => B::LeF32,
            (">", true) => B::GtF32,
            (">=", true) => B::GeF32,
            ("&", false) if ty == &INT => B::BitAnd,
            ("|", false) if ty == &INT => B::BitOr,
            ("^", false) if ty == &INT => B::BitXor,
            ("<<", false) if ty == &INT => B::Shl,
            (">>", false) if ty == &INT => B::Shr,
            _ => return Err(at.error("invalid operation for this type")),
        };
        self.emit(Op::Binary(operation), at);
        if !comparison && ty == &Ty::Scalar(Scalar::Ticks) {
            self.emit(Op::Convert(Conversion::I32ToTicks), at);
        }
        Ok(if comparison { BOOL } else { ty.clone() })
    }
    fn call(
        &mut self,
        name: &str,
        arguments: &[Expr],
        awaited: bool,
        expected: Option<&Ty>,
        at: &Location,
    ) -> Result<Ty, Diagnostic> {
        if name == "Some" {
            if awaited || arguments.len() != 1 {
                return Err(at.error("Some expects one synchronous value"));
            }
            self.emit(Op::Push(1), at);
            let hint = if let Some(Ty::Option(inner)) = expected {
                Some(inner.as_ref())
            } else {
                None
            };
            let inner = self.expr(&arguments[0], hint)?;
            value_type(&inner, at)?;
            return Ok(Ty::Option(Box::new(inner)));
        }
        if matches!(name, "i32" | "f32" | "ticks") {
            if awaited || arguments.len() != 1 {
                return Err(at.error("conversion expects one synchronous argument"));
            }
            let from = self.expr(&arguments[0], None)?;
            let to = match name {
                "i32" => INT,
                "f32" => FLOAT,
                _ => Ty::Scalar(Scalar::Ticks),
            };
            match (from.scalar(), to.scalar()) {
                (Some(Scalar::I32), Some(Scalar::F32)) => {
                    self.emit(Op::Convert(Conversion::I32ToF32), at);
                }
                (Some(Scalar::F32), Some(Scalar::I32)) => {
                    self.emit(Op::Convert(Conversion::F32ToI32), at);
                }
                (Some(Scalar::I32), Some(Scalar::Ticks)) => {
                    self.emit(Op::Convert(Conversion::I32ToTicks), at);
                }
                (Some(Scalar::Ticks), Some(Scalar::I32)) => {}
                _ if from == to => {}
                _ => return Err(at.error("unsupported explicit conversion")),
            }
            return Ok(to);
        }
        for qualified in self.compiler.candidates(&self.function.module, name) {
            if let Some(message) = self.compiler.messages.get(&qualified).cloned() {
                self.compiler
                    .visible(&self.function.module, &qualified, message.public, at)?;
                if awaited {
                    return Err(at.error("message construction cannot be awaited"));
                }
                if arguments.len() != message.parameters.len() {
                    return Err(at.error(format!(
                        "message '{name}' expects {} arguments",
                        message.parameters.len()
                    )));
                }
                self.emit(Op::Push(message.index as i32), at);
                for (argument, ty) in arguments.iter().zip(&message.parameters) {
                    self.expr(argument, Some(ty))?;
                }
                for _ in arguments.len()..MAX_MESSAGE_ARGUMENTS {
                    self.emit(Op::Push(0), at);
                }
                return Ok(Ty::Scalar(Scalar::Message));
            }
            if let Some((enum_name, variant)) = qualified.rsplit_once("::")
                && let Some(variants) = self.compiler.enums.get(enum_name)
                && let Some((tag, (_, parameters))) = variants
                    .iter()
                    .enumerate()
                    .find(|(_, (name, _))| name == variant)
            {
                if awaited {
                    return Err(at.error("enum construction cannot be awaited"));
                }
                let parameters = parameters.clone();
                if parameters.len() != arguments.len() {
                    return Err(at.error("enum variant has the wrong number of arguments"));
                }
                let ty = self.compiler.ty(
                    &self.function.module,
                    &TypeRef::Named(enum_name.into()),
                    at,
                )?;
                self.emit(Op::Push(tag as i32), at);
                let mut used = 1;
                for (value, parameter) in arguments.iter().zip(&parameters) {
                    self.expr(value, Some(parameter))?;
                    used += self.compiler.width(parameter, at)?;
                }
                for _ in used..self.compiler.width(&ty, at)? {
                    self.emit(Op::Push(0), at);
                }
                return Ok(ty);
            }
            if let Some(native) = self
                .compiler
                .natives
                .iter()
                .find(|native| native.name == qualified)
                .copied()
            {
                self.check_await(awaited, native.suspends, at)?;
                if arguments.len() != native.parameters.len() {
                    return Err(at.error(format!(
                        "'{name}' expects {} arguments",
                        native.parameters.len()
                    )));
                }
                for (argument, parameter) in arguments.iter().zip(native.parameters) {
                    self.argument(argument, &native_ty(*parameter))?;
                }
                self.emit(Op::Native(native.opcode), at);
                return Ok(native.result.map(native_ty).unwrap_or(Ty::Unit));
            }
            if let Some(function) = self.compiler.functions.get(&qualified).cloned() {
                self.compiler.visible(
                    &self.function.module,
                    &qualified,
                    function.ast.public,
                    at,
                )?;
                self.check_await(awaited, function.ast.task, at)?;
                self.arguments(&function, arguments, at)?;
                self.emit(Op::CallFunction(function.index), at);
                return Ok(function.result);
            }
        }
        Err(at.error(format!("unknown function '{name}'")))
    }
    fn arguments(
        &mut self,
        function: &Function,
        arguments: &[Expr],
        at: &Location,
    ) -> Result<(), Diagnostic> {
        if arguments.len() != function.parameters.len() {
            return Err(at.error(format!(
                "'{}' expects {} arguments",
                function.ast.name,
                function.parameters.len()
            )));
        }
        for (argument, parameter) in arguments.iter().zip(&function.parameters) {
            self.argument(argument, parameter)?;
        }
        self.compiler
            .calls
            .entry(self.function.index)
            .or_default()
            .insert(function.index);
        Ok(())
    }
    fn argument(&mut self, argument: &Expr, parameter: &Ty) -> Result<(), Diagnostic> {
        self.expr(argument, Some(parameter))?;
        let width = self.compiler.width(parameter, &argument.at)?;
        if width == 1 {
            self.emit(Op::ArgumentValue, &argument.at);
        } else {
            let temporary = self.reserve(parameter, &argument.at)?;
            self.store(temporary, parameter, &argument.at)?;
            for offset in 0..width {
                self.emit(Op::LoadLocal(temporary + offset), &argument.at);
                self.emit(Op::ArgumentValue, &argument.at);
            }
        }
        Ok(())
    }
    fn spawn(&mut self, name: &str, arguments: &[Expr], at: &Location) -> Result<Ty, Diagnostic> {
        if self.in_cleanup {
            return Err(at.error("cleanup cannot spawn tasks"));
        }
        if !self.function.ast.task {
            return Err(at.error("spawn is only valid inside a task"));
        }
        for qualified in self.compiler.candidates(&self.function.module, name) {
            if let Some(function) = self.compiler.functions.get(&qualified).cloned() {
                self.compiler.visible(
                    &self.function.module,
                    &qualified,
                    function.ast.public,
                    at,
                )?;
                if !function.ast.task {
                    return Err(at.error("spawn requires a task, not a synchronous function"));
                }
                self.arguments(&function, arguments, at)?;
                self.emit(Op::SpawnFunction(function.index), at);
                return Ok(Ty::Task(Box::new(function.result)));
            }
        }
        Err(at.error(format!("unknown authored task '{name}'")))
    }
    fn check_await(&self, awaited: bool, suspends: bool, at: &Location) -> Result<(), Diagnostic> {
        if awaited && self.in_cleanup {
            return Err(at.error("cleanup cannot suspend"));
        }
        if awaited && !self.function.ast.task {
            return Err(at.error("await is only valid inside a task"));
        }
        if awaited != suspends {
            return Err(at.error(if suspends {
                "suspendable operation requires await"
            } else {
                "synchronous operation cannot be awaited"
            }));
        }
        Ok(())
    }
}

fn value_type(ty: &Ty, at: &Location) -> Result<(), Diagnostic> {
    if matches!(ty, Ty::Task(_)) {
        Err(at.error("task handles must remain in task-local variables"))
    } else {
        Ok(())
    }
}

fn message_parts(
    text: &str,
    parameters: &[MessageParameter],
    at: &Location,
) -> Result<Vec<MessagePart>, Diagnostic> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '{' if characters.peek() == Some(&'{') => {
                characters.next();
                literal.push('{');
            }
            '}' if characters.peek() == Some(&'}') => {
                characters.next();
                literal.push('}');
            }
            '{' => {
                if !literal.is_empty() {
                    parts.push(MessagePart::Text(std::mem::take(&mut literal)));
                }
                let mut name = String::new();
                loop {
                    match characters.next() {
                        Some('}') => break,
                        Some('{') | None => {
                            return Err(at.error(
                                "unclosed message placeholder; use '{{' for a literal brace",
                            ));
                        }
                        Some(character) => name.push(character),
                    }
                }
                let index = parameters
                    .iter()
                    .position(|parameter| parameter.name == name)
                    .ok_or_else(|| at.error(format!("unknown message placeholder '{{{name}}}'")))?;
                parts.push(MessagePart::Argument(index as u8));
            }
            '}' => return Err(at.error("unmatched message brace; use '}}' for a literal brace")),
            character => literal.push(character),
        }
    }
    if !literal.is_empty() {
        parts.push(MessagePart::Text(literal));
    }
    Ok(parts)
}

fn same(expected: &Ty, actual: &Ty, at: &Location) -> Result<(), Diagnostic> {
    if expected == actual {
        Ok(())
    } else {
        Err(at.error(format!("expected {expected:?}, found {actual:?}")))
    }
}
fn array_length(length: usize, at: &Location) -> Result<u16, Diagnostic> {
    u16::try_from(length).map_err(|_| at.error("array length exceeds u16"))
}
fn number_value(source: &str, at: &Location) -> Result<(Ty, i32), Diagnostic> {
    let text = source.replace('_', "");
    if let Some(ticks) = text.strip_suffix("ticks") {
        let value = ticks
            .parse::<i32>()
            .map_err(|_| at.error("tick literal is outside i32"))?;
        if value < 0 {
            return Err(at.error("tick duration cannot be negative"));
        }
        return Ok((Ty::Scalar(Scalar::Ticks), value));
    }
    if text.contains('.') {
        let value = text
            .parse::<f32>()
            .map_err(|_| at.error("invalid f32 literal"))?;
        if !value.is_finite() {
            return Err(at.error("f32 literal must be finite"));
        }
        return Ok((FLOAT, value.to_bits() as i32));
    }
    let value = if let Some(hex) = text.strip_prefix("0x") {
        i64::from_str_radix(hex, 16)
    } else {
        text.parse::<i64>()
    }
    .ok()
    .and_then(|value| i32::try_from(value).ok())
    .ok_or_else(|| at.error("integer literal is outside i32"))?;
    Ok((INT, value))
}
fn returns(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match &statement.kind {
        S::Return(_) => true,
        S::If(_, yes, no) => returns(yes) && returns(no),
        S::Match(_, arms) => !arms.is_empty() && arms.iter().all(|(_, body)| returns(body)),
        S::Block(body) => returns(body),
        _ => false,
    })
}
fn reaches(
    start: u16,
    at: u16,
    edges: &BTreeMap<u16, BTreeSet<u16>>,
    visited: &mut BTreeSet<u16>,
) -> bool {
    if !visited.insert(at) {
        return false;
    }
    edges.get(&at).is_some_and(|next| {
        next.iter()
            .any(|next| *next == start || reaches(start, *next, edges, visited))
    })
}
