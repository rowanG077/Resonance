use super::global_flow::Origin;
use super::{Context, Definitions, domains, error, literal_width};
use crate::Diagnostic;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};
use symphonia_script::{
    scenario::{self, Instruction},
    semantics::{self, NativeRegistry, OperatorForm},
};

pub struct Decompilation {
    pub source: String,
    pub instructions: usize,
    /// Instructions recovered into expressions, calls and ordinary control flow.
    pub lifted_instructions: usize,
    /// Explicit VM intrinsics used for stack values crossing control-flow edges.
    pub intrinsic_instructions: usize,
    /// Auditable identity operands, excluding scalar coordinates/counts.
    pub identities: Vec<IdentityUse>,
    /// Identities in headerless message expressions use absolute byte locations.
    pub message_identities: Vec<MessageIdentityUse>,
    pub message_controls: usize,
    pub promoted_message_controls: usize,
}

#[derive(Debug, Clone)]
pub struct MessageIdentityUse {
    pub byte_offset: usize,
    pub control: u8,
    pub native: String,
    pub parameter: String,
    pub domain: String,
    pub value: i32,
    pub symbolic: bool,
    pub partial_mask: bool,
}

#[derive(Debug, Clone)]
pub struct IdentityUse {
    pub pc: u32,
    pub native: String,
    pub parameter: String,
    pub domain: String,
    /// The actual operand value, including when represented by a bound formula.
    pub value: i32,
    /// A name or checked bound formula exists; this need not be an enum variant.
    pub symbolic: bool,
    pub partial_mask: bool,
}

struct Rendering {
    imports: BTreeSet<String>,
    identities: Vec<IdentityUse>,
    constant_domains: BTreeMap<u32, ConstantDomain>,
    snapshot_origins: BTreeMap<u32, Origin>,
    reset_slots: BTreeMap<u32, super::bank_reset::Slot>,
    operand_profiles: super::operand_profiles::OperandProfiles,
    context: Context,
    message_control: Option<u8>,
}

struct ConstantDomain {
    name: String,
    bound_before: bool,
}

/// Lift every reachable instruction and preserve the resource's remaining data.
/// Every result is compiled and compared to the input before being returned.
pub fn decompile(bytes: &[u8], definitions: &Definitions) -> Result<Decompilation, Diagnostic> {
    decompile_with_context(bytes, definitions, Context::Unspecified)
}

pub fn decompile_with_context(
    bytes: &[u8],
    definitions: &Definitions,
    context: Context,
) -> Result<Decompilation, Diagnostic> {
    decompile_inner(bytes, definitions, context, None)
}

fn decompile_inner(
    bytes: &[u8],
    definitions: &Definitions,
    context: Context,
    message_control: Option<u8>,
) -> Result<Decompilation, Diagnostic> {
    decompile_with_profiles(
        bytes,
        definitions,
        context,
        super::operand_profiles::OperandProfiles::for_bytes(bytes, context),
        message_control,
    )
}

pub(super) fn decompile_with_profiles(
    bytes: &[u8],
    definitions: &Definitions,
    context: Context,
    operand_profiles: super::operand_profiles::OperandProfiles,
    message_control: Option<u8>,
) -> Result<Decompilation, Diagnostic> {
    symphonia_script::Program::decode(bytes).map_err(|e| error(e.to_string()))?;
    let mut analysis = scenario::analyze(bytes).map_err(|e| error(e.to_string()))?;
    let registry = NativeRegistry::gqseaf();
    let base = analysis.header.code_base();
    // Retail resources retain disabled events and dead branches between live
    // blocks. Recover their canonical instructions too, while keeping actual
    // tables, overlapping registry bytes and unknown encodings as data.
    let original: BTreeSet<_> = analysis.instructions.keys().copied().collect();
    let mut offset = base.max(8 + analysis.records.len() * 12);
    while offset + 2 <= analysis.code_end() {
        let pc = ((offset - base) / 2) as u32;
        if let Some(i) = analysis.instructions.get(&pc) {
            offset += i.size_words as usize * 2;
            continue;
        }
        if analysis
            .instructions
            .range(..pc)
            .next_back()
            .is_some_and(|(_, i)| pc < i.end_pc())
        {
            offset += 2;
            continue;
        }
        let (i, diagnostics) = scenario::decode_at(bytes, pc).map_err(|e| error(e.to_string()))?;
        let end = i.offset + i.size_words as usize * 2;
        let overlaps = analysis.instructions.range(pc..i.end_pc()).next().is_some();
        if !i.raw && diagnostics.is_empty() && !overlaps && end <= analysis.code_end() {
            offset = end;
            analysis.instructions.insert(pc, i);
        } else {
            offset += 2;
        }
    }
    let invalid: Vec<_> = analysis
        .instructions
        .values()
        .filter(|i| {
            !original.contains(&i.pc)
                && matches!(i.mnemonic, "jump" | "call" | "branch_false")
                && !analysis.instructions.contains_key(&(i.operands[0] as u32))
        })
        .map(|i| i.pc)
        .collect();
    for pc in invalid {
        analysis.instructions.remove(&pc);
    }
    let mut labels = BTreeMap::new();
    let mut tasks = BTreeSet::new();
    let mut event_keys = BTreeSet::new();
    for record in analysis.records.iter().filter(|r| r.active()) {
        let kind = match record.kind {
            0 => "actor",
            1 => "region",
            _ => "spawned",
        };
        let name = if event_keys.insert((record.kind, record.key)) {
            format!("{kind}_{}", record.key)
        } else {
            format!("{kind}_{}_r{}", record.key, record.index)
        };
        labels.entry(base + record.pc as usize * 2).or_insert(name);
        tasks.insert(base + record.pc as usize * 2);
    }
    let entry = base + usize::from(analysis.header.default_pc) * 2;
    labels.insert(entry, "entry".into());
    tasks.insert(entry);
    for (&pc, i) in &analysis.instructions {
        if original.contains(&pc) {
            continue;
        }
        if analysis
            .instructions
            .range(..pc)
            .next_back()
            .is_some_and(|(&previous, before)| {
                original.contains(&previous)
                    && before.end_pc() == pc
                    && matches!(before.mnemonic, "ret" | "end")
            })
        {
            labels
                .entry(i.offset)
                .or_insert_with(|| format!("unused_{pc:04X}"));
            tasks.insert(i.offset);
        }
    }
    for i in analysis.instructions.values() {
        if matches!(i.mnemonic, "jump" | "call" | "branch_false") {
            let offset = base + i.operands[0] as usize * 2;
            labels.entry(offset).or_insert_with(|| {
                if i.mnemonic == "call" {
                    format!("sub_{:04X}", i.operands[0])
                } else {
                    format!("L_{:04X}", i.operands[0])
                }
            });
            if i.mnemonic == "call" {
                tasks.insert(offset);
            }
        }
    }
    let auxiliary = analysis.header.auxiliary_offset();
    if analysis.header.auxiliary_words != 0 && auxiliary <= bytes.len() {
        labels
            .entry(auxiliary)
            .or_insert_with(|| "auxiliary".into());
    }
    let code: BTreeMap<usize, &Instruction> = analysis
        .instructions
        .values()
        .map(|i| (i.offset, i))
        .collect();
    let occupied: BTreeSet<usize> = code
        .values()
        .flat_map(|i| i.offset..i.offset + i.size_words as usize * 2)
        .collect();
    // Normal resources have a header followed by ordered event records, then
    // code. Overlapping headers and unknown inactive PCs retain explicit data.
    let structured_events = base == 8 + analysis.records.len() * 12
        && occupied.range(..base).next().is_none()
        && labels.range(..base).next().is_none()
        && analysis
            .records
            .iter()
            .all(|record| labels.contains_key(&(base + record.pc as usize * 2)));
    let mut references = BTreeMap::<usize, (usize, String)>::new();
    let mut reference = |offset: usize, size: usize, kind: &str, target: usize| {
        if !(offset..offset + size).any(|o| occupied.contains(&o))
            && let Some(label) = labels.get(&target)
        {
            references.insert(offset, (size, format!("{kind}({label});")));
        }
    };
    reference(2, 2, "pc", entry);
    if analysis.header.auxiliary_words != 0 {
        reference(4, 2, "offset", auxiliary);
    }
    for record in &analysis.records {
        reference(record.offset + 8, 4, "pc32", base + record.pc as usize * 2);
    }
    let mut scalars = vec![
        (
            0,
            2,
            format!("word({}); // code base", analysis.header.code_base_words),
        ),
        (
            6,
            2,
            format!(
                "word({}); // event registry count",
                analysis.header.registry_count
            ),
        ),
    ];
    let mut imports = BTreeSet::new();
    let mut events = String::new();
    if structured_events {
        writeln!(events, "events {{\n    entry {};", labels[&entry]).unwrap();
        if analysis.header.auxiliary_words != 0 {
            writeln!(events, "    auxiliary {};", labels[&auxiliary]).unwrap();
        }
    }
    for record in &analysis.records {
        let mut scalar = |domain: &str, value: u32| -> Result<String, Diagnostic> {
            definitions.require(domain)?;
            Ok(definitions.format(domain, value as i32).map_or_else(
                || value.to_string(),
                |name| {
                    imports.insert(domain.split("::").next().unwrap().to_owned());
                    name
                },
            ))
        };
        let kind = scalar("scenario::EventKind", record.kind)?;
        let key = if context.is_field() && record.kind == 0 && (1..=10).contains(&record.key) {
            scalar("characters::CharacterId", record.key)?
        } else {
            record.key.to_string()
        };
        if structured_events {
            writeln!(
                events,
                "    on {kind}({key}) => {};",
                labels[&(base + record.pc as usize * 2)]
            )
            .unwrap();
        } else {
            scalars.extend([
                (record.offset, 4, format!("dword({kind}); // event kind")),
                (record.offset + 4, 4, format!("dword({key}); // event key")),
            ]);
        }
    }
    if structured_events {
        events.push_str("}\n\n");
    }
    for (at, size, text) in scalars {
        if !(at..at + size).any(|o| occupied.contains(&o)) {
            references.insert(at, (size, text));
        }
    }
    let auxiliary_text = if analysis.header.auxiliary_words != 0 {
        super::auxiliary::prepare(
            bytes,
            auxiliary,
            &code,
            &occupied,
            &mut labels,
            &mut references,
            (definitions, context),
        )?
    } else {
        super::auxiliary::Plan::default()
    };
    imports.extend(auxiliary_text.imports.iter().cloned());
    let mut body = events;
    let (mut offset, mut in_task, mut in_data, mut intrinsic) =
        (if structured_events { base } else { 0 }, false, false, 0);
    let globals = code
        .values()
        .filter(|i| i.mnemonic == "load.s32")
        .map(|i| i.operands[0])
        .filter(|&address| {
            global_value_domain(address).is_some()
                && definitions
                    .name("story::GlobalId", (address as i32 - 0x40) / 4)
                    .is_some()
        })
        .collect();
    let untyped_loads = code
        .values()
        .filter(|i| operand_profiles.global_role(i).is_some())
        .map(|i| i.pc)
        .collect();
    let snapshot_origins =
        super::global_flow::analyze(&code, &labels, &tasks, &globals, &untyped_loads);
    let mut identities = global_constants(
        &code,
        &labels,
        &snapshot_origins,
        definitions,
        context,
        &untyped_loads,
    );
    for i in code.values() {
        if let Some(literal) = code.get(&(i.offset + i.size_words as usize * 2))
            && let Some(calculate) = code.get(&(literal.offset + literal.size_words as usize * 2))
            && !labels.contains_key(&literal.offset)
            && !labels.contains_key(&calculate.offset)
            && let Some(domain) =
                operand_profiles.flag_constant_domain(i, literal, calculate, definitions)
        {
            identities.push(IdentityUse {
                pc: i.pc,
                native: "memory::flag_constant32".into(),
                parameter: format!("{:#x}/0x10", i.operands[0]),
                domain: domain.into(),
                value: literal.operands[0] as i32,
                symbolic: true,
                partial_mask: false,
            });
        }
        if let Some(domain) = operand_profiles.global_role(i) {
            let value = (i.operands[0] as i32 - 0x40) / 4;
            if definitions.name(domain, value).is_some() {
                identities.push(IdentityUse {
                    pc: i.pc,
                    native: "memory::global_alias".into(),
                    parameter: format!("{:#x}", i.operands[0]),
                    domain: domain.into(),
                    value,
                    symbolic: true,
                    partial_mask: false,
                });
            }
        }
    }
    let mut constant_domains = BTreeMap::new();
    for usage in &identities {
        let bound_before = usage.symbolic && definitions.name(&usage.domain, usage.value).is_none();
        if matches!(
            usage.native.as_str(),
            "memory::snapshot32" | "memory::battle_outcome" | "memory::flag_constant32"
        ) || bound_before
        {
            let load = &analysis.instructions[&usage.pc];
            let literal = code[&(load.offset + load.size_words as usize * 2)];
            constant_domains.insert(
                literal.pc,
                ConstantDomain {
                    name: usage.domain.clone(),
                    bound_before,
                },
            );
        }
    }
    for mut usage in super::identity_flow::analyze(&code, &labels, &registry) {
        if usage.domain == "story::StoryFlag"
            && let Some(domain) = definitions.scoped_flag_domain(context, usage.value)
        {
            usage.domain = domain.into();
        }
        definitions.require(&usage.domain)?;
        let value = usage.value;
        let symbolic = definitions.name(&usage.domain, value).is_some();
        constant_domains.insert(
            usage.constant_pc,
            ConstantDomain {
                name: usage.domain.clone(),
                bound_before: false,
            },
        );
        identities.push(IdentityUse {
            pc: usage.constant_pc,
            native: "memory::copy".into(),
            parameter: format!(
                "{}.{} via {:#X} at {:#X}",
                usage.native, usage.parameter, usage.address, usage.use_pc
            ),
            domain: usage.domain,
            value,
            symbolic,
            partial_mask: false,
        });
    }
    let mut rendering = Rendering {
        imports,
        identities,
        constant_domains,
        snapshot_origins,
        reset_slots: super::bank_reset::analyze(&code, &labels, &registry, definitions),
        operand_profiles,
        context,
        message_control,
    };
    while offset < bytes.len() {
        if let Some(label) = labels.get(&offset) {
            if in_data {
                body.push_str("}\n\n");
                in_data = false;
            }
            if tasks.contains(&offset) || !code.contains_key(&offset) {
                if in_task {
                    body.push_str("}\n\n");
                    in_task = false;
                }
                if code.contains_key(&offset) {
                    writeln!(body, "pub task {label}() {{").unwrap();
                    in_task = true;
                } else {
                    writeln!(body, "{label}:").unwrap();
                }
            } else {
                writeln!(body, "    {label}:").unwrap();
            }
        }
        if let Some(i) = code.get(&offset) {
            if in_data {
                body.push_str("}\n\n");
                in_data = false;
            }
            if !in_task {
                writeln!(body, "task code_{:04X}() {{", i.pc).unwrap();
                in_task = true;
            }
            let identity_count = rendering.identities.len();
            if let Some((end, text)) = lift(
                offset,
                &code,
                &labels,
                &registry,
                definitions,
                &mut rendering,
            ) {
                writeln!(body, "    {text}").unwrap();
                offset = end;
                continue;
            }
            rendering.identities.truncate(identity_count);
            let target = || &labels[&(base + i.operands[0] as usize * 2)];
            let text = match i.mnemonic {
                "jump" => format!("goto {};", target()),
                "call" => format!("call {};", target()),
                "branch_false" => format!("if_false (result) goto {};", target()),
                "ret" => "return;".into(),
                "end" => "end;".into(),
                "push.s8" | "push.s16" | "push.u16" | "push.s32" => {
                    intrinsic += 1;
                    format!(
                        "push {};",
                        literal(
                            i,
                            rendering.constant_domains.get(&i.pc),
                            definitions,
                            &mut rendering.imports
                        )
                        .text
                    )
                }
                "load.s8" | "load.s16" | "load.s32" => {
                    intrinsic += 1;
                    format!(
                        "push {};",
                        memory(
                            i,
                            definitions,
                            &rendering.snapshot_origins,
                            &rendering.operand_profiles,
                            &mut rendering.imports
                        )
                        .text
                    )
                }
                "calc" => {
                    intrinsic += 1;
                    format!(
                        "vm::calculate({});",
                        semantics::calculator_name(i.operands[0] as u8).unwrap()
                    )
                }
                "arg" => {
                    intrinsic += 1;
                    "vm::argument();".into()
                }
                "proc" => {
                    intrinsic += 1;
                    let name = registry
                        .get(i.operands[0] as u8)
                        .map(|n| n.name.clone())
                        .unwrap_or_else(|| format!("0x{:02X}", i.operands[0]));
                    format!("vm::native({name});")
                }
                _ => return Err(error(format!("unsupported instruction {}", i.mnemonic))),
            };
            writeln!(body, "    {text}").unwrap();
            offset += i.size_words as usize * 2;
        } else {
            if in_task {
                body.push_str("}\n\n");
                in_task = false;
            }
            if !in_data {
                body.push_str("data {\n");
                in_data = true;
            }
            if let Some((size, text)) = references.get(&offset) {
                writeln!(body, "    {text}").unwrap();
                offset += size;
            } else if let Some((size, text)) = auxiliary_text.data.get(&offset) {
                writeln!(body, "    {text}").unwrap();
                offset += size;
            } else {
                let start = offset;
                offset += 1;
                while offset < bytes.len()
                    && offset - start < 32
                    && !code.contains_key(&offset)
                    && !references.contains_key(&offset)
                    && !auxiliary_text.data.contains_key(&offset)
                    && !labels.contains_key(&offset)
                {
                    offset += 1;
                }
                let mut hex = String::new();
                for b in &bytes[start..offset] {
                    write!(hex, "{b:02X}").unwrap();
                }
                writeln!(body, "    hex(\"{hex}\");").unwrap();
            }
        }
    }
    if in_task || in_data {
        body.push_str("}\n");
    }
    if let Some(label) = labels.get(&bytes.len()) {
        writeln!(body, "{label}:").unwrap();
    }
    // An absent definitions file is a configuration error, not evidence that
    // the original identity is unknown. Never silently downgrade it to a number.
    for identity in &rendering.identities {
        definitions.require(&identity.domain)?;
    }
    for i in analysis.instructions.values() {
        if i.mnemonic == "load.s32"
            && (0x40..0x440).contains(&i.operands[0])
            && i.operands[0] % 4 == 0
        {
            definitions.require("story::GlobalId")?;
            if definitions
                .name("story::GlobalId", (i.operands[0] as i32 - 0x40) / 4)
                .is_some()
                && !untyped_loads.contains(&i.pc)
                && let Some(domain) = global_value_domain(i.operands[0])
            {
                definitions.require(domain)?;
            }
        }
        if i.mnemonic == "proc" && matches!(i.operands[0], 0x98 | 0x9b) && context.is_field() {
            definitions.require("resources::FieldAnimationId")?;
            definitions.require("resources::FieldModelId")?;
        }
    }
    let mut source = String::from("script scenario;\n");
    for import in rendering.imports {
        writeln!(source, "use std::{import};").unwrap();
    }
    source
        .push_str("\n// Original VM: eager expressions, shared memory, native event scheduling.\n");
    if !structured_events {
        writeln!(source, "code_base = {};\n", analysis.header.code_base_words).unwrap();
    }
    source.push_str(&wrap_calls(&structure(&body)));
    let restored = super::compile("scenario", &source, definitions)?;
    if restored != bytes {
        let at = restored
            .iter()
            .zip(bytes)
            .position(|(a, b)| a != b)
            .unwrap_or(restored.len().min(bytes.len()));
        return Err(error(format!(
            "decompiled source differs at byte {at:#X} ({} vs {} bytes)",
            restored.len(),
            bytes.len()
        )));
    }
    Ok(Decompilation {
        source,
        instructions: code.len(),
        lifted_instructions: code.len() - intrinsic,
        intrinsic_instructions: intrinsic,
        identities: rendering.identities,
        message_identities: auxiliary_text.identities,
        message_controls: auxiliary_text.controls,
        promoted_message_controls: auxiliary_text.promoted_controls,
    })
}

#[derive(Clone)]
struct Expression {
    text: String,
    literal: Option<(i32, String)>,
    value_domain: Option<String>,
    /// A temporary snapshot has a comparison domain, not a permanent slot type.
    snapshot_domain: Option<String>,
    binary: Option<(u8, &'static str, Box<Expression>, Box<Expression>)>,
    unary: Option<(u8, Box<Expression>)>,
}
impl Expression {
    fn text(text: String) -> Self {
        Self {
            text,
            literal: None,
            value_domain: None,
            snapshot_domain: None,
            binary: None,
            unary: None,
        }
    }
    fn binary(opcode: u8, symbol: &'static str, left: Self, right: Self) -> Self {
        Self {
            text: format!("({} {symbol} {})", left.text, right.text),
            literal: None,
            value_domain: None,
            snapshot_domain: None,
            binary: Some((opcode, symbol, Box::new(left), Box::new(right))),
            unary: None,
        }
    }
    fn prefix(opcode: u8, symbol: &str, value: Self) -> Self {
        Self {
            text: format!("{symbol}({})", value.text),
            literal: None,
            value_domain: None,
            snapshot_domain: None,
            binary: None,
            unary: Some((opcode, Box::new(value))),
        }
    }
    fn constant_value(&self) -> Option<i32> {
        if let Some((n, _)) = self.literal {
            return Some(n);
        }
        if let Some((op, value)) = &self.unary {
            return super::constant::unary(*op, value.constant_value()?);
        }
        let (op, _, left, right) = self.binary.as_ref()?;
        super::constant::binary(*op, left.constant_value()?, right.constant_value()?)
    }
    fn indexed_base<'a>(
        &self,
        domain: Option<&str>,
        definitions: &'a Definitions,
        context: Context,
    ) -> Option<(&'a str, i32)> {
        if domain != Some("story::StoryFlag") {
            return None;
        }
        let (0x31, _, left, right) = self.binary.as_ref()? else {
            return None;
        };
        let (base, offset) = match (left.constant_value(), right.constant_value()) {
            (Some(base), None) => (base, right),
            (None, Some(base)) => (base, left),
            _ => return None,
        };
        // An individual flag with the same number does not establish a family.
        // Only the supplied definitions can declare a reviewed additive base.
        let base_domain = definitions
            .scoped_flag_base_domain(context, base)
            .unwrap_or("story::StoryFlagBase");
        if definitions.name(base_domain, base).is_some() {
            Some((base_domain, base))
        } else {
            // Keep the original additions: ((base + index) + slot_bit) is
            // still an indexed family. Only descend a chain of additions
            // with one constant side; do not cross other operations or two
            // dynamic operands. A declared outer base retains precedence.
            offset.indexed_base(domain, definitions, context)
        }
    }
    fn name_indexed_base(
        &mut self,
        domain: &str,
        base: i32,
        definitions: &Definitions,
        imports: &mut BTreeSet<String>,
    ) {
        let Some((0x31, symbol, left, right)) = &mut self.binary else {
            return;
        };
        if left.constant_value() == Some(base) {
            left.domain(Some(domain), definitions, imports);
        } else if right.constant_value() == Some(base) {
            right.domain(Some(domain), definitions, imports);
        } else if left.constant_value().is_some() {
            right.name_indexed_base(domain, base, definitions, imports);
        } else {
            left.name_indexed_base(domain, base, definitions, imports);
        }
        self.text = format!("({} {symbol} {})", left.text, right.text);
    }
    fn treasure_reward(
        &mut self,
        definitions: &Definitions,
        imports: &mut BTreeSet<String>,
    ) -> bool {
        if let Some((0x31, symbol, left, right)) = &mut self.binary
            && left.literal.as_ref().is_some_and(|(n, _)| *n == 1024)
        {
            left.domain(
                Some("treasure::TreasureRewardEncoding"),
                definitions,
                imports,
            );
            self.text = format!("({} {symbol} {})", left.text, right.text);
            true
        } else {
            false
        }
    }
    fn domain(
        &mut self,
        domain: Option<&str>,
        definitions: &Definitions,
        imports: &mut BTreeSet<String>,
    ) {
        self.domain_or_bound(domain, false, definitions, imports);
    }
    fn domain_or_bound(
        &mut self,
        domain: Option<&str>,
        allow_bound_before: bool,
        definitions: &Definitions,
        imports: &mut BTreeSet<String>,
    ) {
        self.domain_or_bound_in_context(
            domain,
            allow_bound_before,
            definitions,
            imports,
            Context::Unspecified,
        );
    }
    fn domain_or_bound_in_context(
        &mut self,
        domain: Option<&str>,
        allow_bound_before: bool,
        definitions: &Definitions,
        imports: &mut BTreeSet<String>,
        context: Context,
    ) {
        if let (Some(domain), Some(value)) = (domain, self.constant_value())
            && let Some(name) = definitions.format(domain, value).or_else(|| {
                allow_bound_before
                    .then(|| definitions.bound_before(domain, value))
                    .flatten()
            })
        {
            imports.insert(domain.split("::").next().unwrap().into());
            if let Some((_, width)) = &self.literal {
                self.text = if width == literal_width(value) {
                    name
                } else {
                    format!("{width}({name})")
                };
            } else if matches!(domain, "scenario::DialogueFlag" | "scenario::AnimationFlag")
                && let Some((0x37, symbol, left, right)) = &mut self.binary
            {
                left.domain(Some(domain), definitions, imports);
                right.domain(Some(domain), definitions, imports);
                self.text = format!("({} {symbol} {})", left.text, right.text);
            } else {
                // Replacing a constant calculator tree with a push would change
                // matching bytecode. The symbol is checked against the retained
                // encoding, without emitting an extra literal or operation.
                self.text = format!("encoded({name}, {})", self.text);
            }
        } else if let Some((base_domain, base)) = self.indexed_base(domain, definitions, context) {
            self.name_indexed_base(base_domain, base, definitions, imports);
        }
    }
}
fn literal(
    i: &Instruction,
    domain: Option<&ConstantDomain>,
    definitions: &Definitions,
    imports: &mut BTreeSet<String>,
) -> Expression {
    let n = i.operands[0] as i32;
    let width = i.mnemonic.strip_prefix("push.").unwrap();
    let mut expression = Expression {
        text: if literal_width(n) == width {
            n.to_string()
        } else {
            format!("{width}({n})")
        },
        literal: Some((n, width.into())),
        value_domain: None,
        snapshot_domain: None,
        binary: None,
        unary: None,
    };
    expression.domain_or_bound(
        domain.map(|d| d.name.as_str()),
        domain.is_some_and(|d| d.bound_before),
        definitions,
        imports,
    );
    expression
}
fn global_value_domain(address: i64) -> Option<&'static str> {
    match address {
        0x40 => Some("story::StoryStage"),
        0x4c => Some("story::ColetteAngelStage"),
        0x50 => Some("world::MusicBankId"),
        0x124 => Some("characters::CharacterId"),
        0x128 => Some("story::KateRescueStage"),
        0x12c => Some("story::ManaLeafHerbStage"),
        0x13c => Some("story::SideEventState"),
        0xb8 => Some("story::PalmacostaRanchAssaultStage"),
        0xbc => Some("story::PalmacostaRanchDorrStage"),
        0xc0 => Some("story::FireSealStage"),
        0xc4 => Some("story::WaterSealStage"),
        0xc8 => Some("story::WindSealStage"),
        0xcc => Some("story::LightSealStage"),
        0xd0 => Some("story::BoltzmanTowerKeyStage"),
        0xd4 => Some("story::PietroInfiltrationStage"),
        0xd8 => Some("story::AsgardRanchEarlyRouteStage"),
        0xdc => Some("story::AsgardRanchLaterRouteStage"),
        0xe0 => Some("story::AsgardRanchStage"),
        0xe4 => Some("story::BookOfRegenerationStage"),
        0xe8 => Some("story::CelsiusPactStage"),
        0xec => Some("story::GnomePactStage"),
        0xf0 => Some("story::ShadowPactStage"),
        0xf8 => Some("story::LuinRebuildingStage"),
        0x130 => Some("story::LinkiteTreeStage"),
        // Counters and newly recovered words are not generic milestones.
        _ => None,
    }
}

/// Report direct globals and must-proven snapshots. Snapshot slots have no
/// permanent type: only comparisons are identities, never overwriting literals.
fn global_constants(
    code: &BTreeMap<usize, &Instruction>,
    labels: &BTreeMap<usize, String>,
    snapshot_origins: &BTreeMap<u32, Origin>,
    definitions: &Definitions,
    context: Context,
    untyped_loads: &BTreeSet<u32>,
) -> Vec<IdentityUse> {
    code.values()
        .filter_map(|load| {
            if load.mnemonic != "load.s32" || untyped_loads.contains(&load.pc) {
                return None;
            }
            let address = load.operands[0];
            let origin = snapshot_origins
                .get(&load.pc)
                .copied()
                .unwrap_or(Origin::Global(address));
            let snapshot = origin != Origin::Global(address);
            let domain = match origin {
                Origin::Global(origin) => {
                    definitions.name("story::GlobalId", (origin as i32 - 0x40) / 4)?;
                    global_value_domain(origin)?
                }
                Origin::BattleOutcome => "battle::BattleOutcome",
            };
            let literal = code.get(&(load.offset + load.size_words as usize * 2))?;
            let calculate = code.get(&(literal.offset + literal.size_words as usize * 2))?;
            if !literal.mnemonic.starts_with("push.")
                || calculate.mnemonic != "calc"
                || !matches!(calculate.operands[0], 0x10 | 0x20..=0x25)
                || (snapshot && calculate.operands[0] == 0x10)
                || labels.contains_key(&literal.offset)
                || labels.contains_key(&calculate.offset)
            {
                return None;
            }
            let value = literal.operands[0] as i32;
            let bound_before = calculate.operands[0] == 0x22;
            let domain = definitions.scoped_comparison_domain(
                Some(domain),
                context,
                Some(value),
                bound_before,
            )?;
            Some(IdentityUse {
                pc: load.pc,
                native: if origin == Origin::BattleOutcome {
                    "memory::battle_outcome"
                } else if snapshot {
                    "memory::snapshot32"
                } else {
                    "memory::constant32"
                }
                .into(),
                parameter: match origin {
                    Origin::BattleOutcome => {
                        format!("{address:#x} from battle/{:#x}", calculate.operands[0])
                    }
                    Origin::Global(origin) if snapshot => {
                        format!("{address:#x} from {origin:#x}/{:#x}", calculate.operands[0])
                    }
                    _ => format!("{address:#x}/{:#x}", calculate.operands[0]),
                },
                domain: domain.into(),
                value,
                symbolic: definitions.name(domain, value).is_some()
                    || (bound_before && definitions.bound_before(domain, value).is_some()),
                partial_mask: false,
            })
        })
        .collect()
}

fn memory(
    i: &Instruction,
    definitions: &Definitions,
    snapshot_origins: &BTreeMap<u32, Origin>,
    profiles: &super::operand_profiles::OperandProfiles,
    imports: &mut BTreeSet<String>,
) -> Expression {
    let address = i.operands[0];
    let role = profiles.global_role(i);
    let index = (address as i32 - 0x40) / 4;
    let name = role
        .and_then(|domain| definitions.name(domain, index))
        .or_else(|| definitions.name("story::GlobalId", index));
    if i.mnemonic == "load.s32"
        && (0x40..0x440).contains(&address)
        && address % 4 == 0
        && let Some(name) = name
    {
        imports.insert("story".into());
        return Expression {
            text: format!("globals[{name}]"),
            literal: None,
            value_domain: if role.is_some() {
                None
            } else {
                global_value_domain(address).map(str::to_owned)
            },
            snapshot_domain: None,
            binary: None,
            unary: None,
        };
    }
    // Address aliases are optional and width-specific. They name shared cells,
    // without giving the values in those cells an identity domain.
    let offset_domain = match i.mnemonic {
        "load.s8" => Some("story::ByteOffset"),
        "load.s16" => Some("story::HalfwordOffset"),
        _ => None,
    };
    let offset = offset_domain
        .and_then(|domain| definitions.name(domain, address as i32))
        .map_or_else(
            || format!("0x{address:04X}"),
            |name| {
                imports.insert("story".into());
                name.to_owned()
            },
        );
    let mut expression = Expression::text(format!("memory::i{}[{offset}]", &i.mnemonic[6..]));
    expression.snapshot_domain = snapshot_origins
        .get(&i.pc)
        .and_then(|&origin| match origin {
            Origin::Global(origin) => global_value_domain(origin),
            Origin::BattleOutcome => Some("battle::BattleOutcome"),
        })
        .map(str::to_owned);
    expression
}

/// Recognize an exactly contiguous expression/call tree. Labels are barriers:
/// values crossing an incoming edge must remain explicit VM operations.
fn lift(
    start: usize,
    code: &BTreeMap<usize, &Instruction>,
    labels: &BTreeMap<usize, String>,
    registry: &NativeRegistry,
    definitions: &Definitions,
    rendering: &mut Rendering,
) -> Option<(usize, String)> {
    let mut values: Vec<Expression> = Vec::new();
    let mut arguments: Vec<Expression> = Vec::new();
    let mut offset = start;
    // Limits keep malformed or very large blocks from causing quadratic scans.
    for _ in 0..1024 {
        if offset != start && labels.contains_key(&offset) {
            return None;
        }
        let i = code.get(&offset)?;
        offset += i.size_words as usize * 2;
        match i.mnemonic {
            "push.s8" | "push.s16" | "push.s32" | "push.u16" => values.push(literal(
                i,
                rendering.constant_domains.get(&i.pc),
                definitions,
                &mut rendering.imports,
            )),
            "load.s8" | "load.s16" | "load.s32" => values.push(memory(
                i,
                definitions,
                &rendering.snapshot_origins,
                &rendering.operand_profiles,
                &mut rendering.imports,
            )),
            "calc" if i.operands[0] == 0 => {
                let mut expression = values.pop().or_else(|| {
                    // The retail interpreter aliases its depth word immediately
                    // below an empty operand stack. This exact sequence passes
                    // -1 as a native argument, then restores depth with a push.
                    // Keep the original operations while expressing the value.
                    let after = code.get(&offset)?;
                    let push = code.get(&(offset + 2))?;
                    let end = code.get(&(offset + 2 + push.size_words as usize * 2))?;
                    (after.mnemonic == "arg"
                        && push.mnemonic.starts_with("push.")
                        && end.mnemonic == "calc"
                        && end.operands == [0]
                        && !labels.contains_key(&offset)
                        && !arguments.is_empty())
                    .then(|| Expression::text("stack_depth()".into()))
                })?;
                if code.get(&offset).is_some_and(|i| i.mnemonic == "arg")
                    && !labels.contains_key(&offset)
                {
                    offset += 2;
                    arguments.push(expression);
                } else if values.is_empty() && arguments.is_empty() {
                    if let Some(branch) = code
                        .get(&offset)
                        .filter(|i| i.mnemonic == "branch_false" && !labels.contains_key(&offset))
                    {
                        // PC targets are relative to the same code base as i.
                        let target = i.offset.checked_sub(i.pc as usize * 2)?
                            + branch.operands[0] as usize * 2;
                        return Some((
                            offset + 4,
                            format!(
                                "if_false ({}) goto {};",
                                expression.text,
                                labels.get(&target)?
                            ),
                        ));
                    }
                    if let Some(control) = rendering.message_control {
                        let domain = match control {
                            1 if expression.constant_value() == Some(999999) => {
                                Some("scenario::ActorSelector")
                            }
                            1 => Some("characters::CharacterId"),
                            4 => Some("items::ItemId"),
                            9 if matches!(expression.constant_value(), Some(-1 | 0)) => {
                                Some("scenario::VoiceControl")
                            }
                            // A high-nibble8 reference is the native direct-stream
                            // path, not an archive/member reference.
                            9 if expression
                                .constant_value()
                                .is_some_and(|value| value as u32 & 0xf0000000 == 0x80000000) =>
                            {
                                None
                            }
                            9 => Some("resources::VoiceArchiveReference"),
                            17 => Some("characters::TitleId"),
                            18 => Some("techniques::TechniqueId"),
                            _ => None,
                        };
                        if let (Some(domain), Some(value)) = (domain, expression.constant_value()) {
                            rendering.identities.push(IdentityUse {
                                pc: i.pc,
                                native: "message::value".into(),
                                parameter: "value".into(),
                                domain: domain.into(),
                                value,
                                symbolic: definitions.format(domain, value).is_some(),
                                partial_mask: false,
                            });
                        }
                        expression.domain(domain, definitions, &mut rendering.imports);
                    }
                    return Some((offset, format!("eval {};", expression.text)));
                } else {
                    return None;
                }
            }
            "calc" => {
                let spec = semantics::calculator(i.operands[0] as u8)?;
                let mut expression = match spec.form {
                    OperatorForm::Prefix | OperatorForm::Noop => {
                        let value = values.pop()?;
                        let symbol = match spec.opcode {
                            3 => "++",
                            4 => "--",
                            _ => spec.symbol,
                        };
                        Expression::prefix(spec.opcode, symbol, value)
                    }
                    OperatorForm::Postfix => {
                        let value = values.pop()?;
                        Expression::text(format!(
                            "({}){}",
                            value.text,
                            if spec.opcode == 1 { "++" } else { "--" }
                        ))
                    }
                    OperatorForm::Index => {
                        let right = values.pop()?;
                        let left = values.pop()?;
                        Expression::text(format!("({})[{}]", left.text, right.text))
                    }
                    OperatorForm::Binary
                    | OperatorForm::Assignment
                    | OperatorForm::CompoundAssignment => {
                        let mut right = values.pop()?;
                        let mut left = values.pop()?;
                        if matches!(spec.opcode, 0x10 | 0x20..=0x25) {
                            let right_domain = definitions.scoped_comparison_domain(
                                left.value_domain.as_deref().or_else(|| {
                                    (spec.opcode != 0x10)
                                        .then_some(left.snapshot_domain.as_deref())
                                        .flatten()
                                }),
                                rendering.context,
                                right.constant_value(),
                                spec.opcode == 0x22,
                            );
                            right.domain_or_bound(
                                right_domain,
                                spec.opcode == 0x22,
                                definitions,
                                &mut rendering.imports,
                            );
                            let left_domain = definitions.scoped_comparison_domain(
                                right.value_domain.as_deref().or_else(|| {
                                    (spec.opcode != 0x10)
                                        .then_some(right.snapshot_domain.as_deref())
                                        .flatten()
                                }),
                                rendering.context,
                                left.constant_value(),
                                spec.opcode == 0x23,
                            );
                            left.domain_or_bound(
                                left_domain,
                                spec.opcode == 0x23,
                                definitions,
                                &mut rendering.imports,
                            );
                        }
                        Expression::binary(spec.opcode, spec.symbol, left, right)
                    }
                    _ => return None,
                };
                let annotation = rendering.constant_domains.get(&i.pc);
                expression.domain_or_bound(
                    annotation.map(|d| d.name.as_str()),
                    annotation.is_some_and(|d| d.bound_before),
                    definitions,
                    &mut rendering.imports,
                );
                values.push(expression);
            }
            "proc" => {
                let native = registry.get(i.operands[0] as u8)?;
                let at = arguments.len().checked_sub(native.arguments.len())?;
                let mut args = arguments.split_off(at);
                let argument_values: Vec<_> = args.iter().map(Expression::constant_value).collect();
                for (index, arg) in args.iter_mut().enumerate() {
                    if index == 0
                        && let Some(slot) = rendering.reset_slots.get(&i.pc)
                        && let Some((value, width)) = arg.literal.as_ref()
                        && *value == slot.base + slot.index
                    {
                        let domain = "story::StoryFlagBase";
                        let name = definitions.name(domain, slot.base)?;
                        let text = format!("flag_slot({name}, {})", slot.index);
                        arg.text = if width == literal_width(*value) {
                            text
                        } else {
                            format!("{width}({text})")
                        };
                        rendering.imports.insert("story".into());
                        rendering.identities.push(IdentityUse {
                            pc: i.pc,
                            native: format!("bank_reset::{}", native.name),
                            parameter: format!("{}[{}]", native.arguments[index], slot.index),
                            domain: domain.into(),
                            value: slot.base,
                            symbolic: true,
                            partial_mask: false,
                        });
                        continue;
                    }
                    let gald_reward = native.opcode == 0x35
                        && index == 1
                        && arg.treasure_reward(definitions, &mut rendering.imports);
                    if gald_reward {
                        rendering.identities.push(IdentityUse {
                            pc: i.pc,
                            native: native.name.clone(),
                            parameter: "reward_base".into(),
                            domain: "treasure::TreasureRewardEncoding".into(),
                            value: 1024,
                            symbolic: definitions
                                .name("treasure::TreasureRewardEncoding", 1024)
                                .is_some(),
                            partial_mask: false,
                        });
                    }
                    let domain = (!gald_reward)
                        .then(|| {
                            rendering
                                .operand_profiles
                                .domain(i.pc, native, index, arg.constant_value(), definitions)
                                .or_else(|| {
                                    domains::argument(
                                        native,
                                        index,
                                        &argument_values,
                                        rendering.context,
                                        definitions,
                                    )
                                })
                        })
                        .flatten();
                    if let (Some(domain), Some(value)) = (domain, arg.constant_value()) {
                        let formatted = definitions.format(domain, value);
                        rendering.identities.push(IdentityUse {
                            pc: i.pc,
                            native: native.name.clone(),
                            parameter: native.arguments[index].clone(),
                            domain: domain.into(),
                            value,
                            symbolic: formatted.is_some(),
                            partial_mask: formatted.is_some_and(|s| s.contains("0x")),
                        });
                    } else if let Some((domain, value)) =
                        arg.indexed_base(domain, definitions, rendering.context)
                    {
                        rendering.identities.push(IdentityUse {
                            pc: i.pc,
                            native: format!("indexed::{}", native.name),
                            parameter: native.arguments[index].clone(),
                            domain: domain.into(),
                            value,
                            symbolic: true,
                            partial_mask: false,
                        });
                    }
                    arg.domain_or_bound_in_context(
                        domain,
                        false,
                        definitions,
                        &mut rendering.imports,
                        rendering.context,
                    );
                }
                let text = format!(
                    "native::{}({})",
                    native.name,
                    args.iter()
                        .zip(&native.arguments)
                        .map(|(a, name)| {
                            if name.starts_with("arg") && name[3..].parse::<u32>().is_ok() {
                                a.text.clone()
                            } else {
                                format!("{name}: {}", a.text)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                if native.returns_value == Some(true) {
                    let mut expression = Expression::text(text);
                    expression.value_domain =
                        domains::result(native, &argument_values).map(str::to_owned);
                    values.push(expression);
                } else if values.is_empty() && arguments.is_empty() {
                    return Some((offset, format!("{text};")));
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
    None
}

/// Recover lexical if/while regions without changing instruction order. The
/// remaining labels retain non-structured edges, including shared switch tails.
fn structure(source: &str) -> String {
    let lines: Vec<_> = source.lines().collect();
    let mut output = String::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if !(line.starts_with("task ") || line.starts_with("pub task ")) {
            writeln!(output, "{line}").unwrap();
            index += 1;
            continue;
        }
        writeln!(output, "{line}").unwrap();
        index += 1;
        let start = index;
        let end = (start..lines.len())
            .find(|&i| lines[i] == "}")
            .unwrap_or(lines.len());
        let labels: BTreeMap<_, _> = (start..end)
            .filter_map(|i| lines[i].trim().strip_suffix(':').map(|s| (s, i)))
            .collect();
        // close index, whether the closing brace emits a backward jump
        let mut regions: Vec<(usize, bool)> = Vec::new();
        while index < end {
            let mut skip = false;
            while regions.last().is_some_and(|&(close, _)| close == index) {
                let (_, is_loop) = regions.pop().unwrap();
                writeln!(output, "{}}}", "    ".repeat(1 + regions.len())).unwrap();
                skip |= is_loop;
            }
            if skip {
                index += 1;
                continue;
            }
            let text = lines[index].trim();
            if let Some((condition, target)) = text
                .strip_prefix("if_false (")
                .and_then(|s| s.rsplit_once(") goto "))
                .and_then(|(c, t)| t.strip_suffix(';').map(|t| (c, t)))
                && let Some(&target_index) = labels.get(target)
                && target_index > index
                && regions
                    .last()
                    .is_none_or(|&(close, _)| target_index <= close)
            {
                let loop_label = index.checked_sub(1).and_then(|i| {
                    lines[i].trim().strip_suffix(':').or_else(|| {
                        (index == start)
                            .then(|| lines[i].split("task ").nth(1)?.split('(').next())
                            .flatten()
                    })
                });
                let is_loop = loop_label.is_some_and(|label| {
                    lines[target_index - 1].trim() == format!("goto {label};")
                });
                writeln!(
                    output,
                    "{}{} ({condition}) {{",
                    "    ".repeat(1 + regions.len()),
                    if is_loop { "while" } else { "if" }
                )
                .unwrap();
                regions.push((
                    if is_loop {
                        target_index - 1
                    } else {
                        target_index
                    },
                    is_loop,
                ));
            } else {
                writeln!(output, "{}{text}", "    ".repeat(1 + regions.len())).unwrap();
            }
            index += 1;
        }
        // Only lexical regions ending at a real label are opened above.
        debug_assert!(regions.is_empty());
    }
    output
}

/// Keep the large native configuration calls useful to edit one parameter at a
/// time. Nested calls stay intact; splitting does not change token order.
pub(crate) fn wrap_calls(source: &str) -> String {
    let mut output = String::new();
    for line in source.lines() {
        let text = line.trim_start();
        if line.len() <= 120 || !(text.starts_with("native::") || text.starts_with("eval native::"))
        {
            writeln!(output, "{line}").unwrap();
            continue;
        }
        let Some(open) = line.find('(') else {
            writeln!(output, "{line}").unwrap();
            continue;
        };
        let Some(close) = line.strip_suffix(");").map(str::len) else {
            writeln!(output, "{line}").unwrap();
            continue;
        };
        let (mut depth, mut start) = (0, open + 1);
        let mut arguments = Vec::new();
        for (index, character) in line[open + 1..close].char_indices() {
            let index = index + open + 1;
            match character {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    arguments.push(line[start..index].trim());
                    start = index + 1;
                }
                _ => {}
            }
        }
        arguments.push(line[start..close].trim());
        if arguments.len() < 2 {
            writeln!(output, "{line}").unwrap();
            continue;
        }
        let indent = &line[..line.len() - text.len()];
        writeln!(output, "{}(", &line[..open]).unwrap();
        for (index, argument) in arguments.iter().enumerate() {
            writeln!(
                output,
                "{indent}    {argument}{}",
                if index + 1 == arguments.len() {
                    ""
                } else {
                    ","
                }
            )
            .unwrap();
        }
        writeln!(output, "{indent});").unwrap();
    }
    output
}

pub(super) struct MessageExpression {
    pub text: String,
    pub imports: BTreeSet<String>,
    pub identities: Vec<IdentityUse>,
}

pub(super) fn message_expression(
    bytes: &[u8],
    control: u8,
    definitions: &Definitions,
    context: Context,
) -> Result<Option<MessageExpression>, Diagnostic> {
    let mut wrapper = vec![0, 4, 0, 0, 0, 0, 0, 0];
    wrapper.extend_from_slice(bytes);
    let result = decompile_inner(&wrapper, definitions, context, Some(control))?;
    let Some((_, body)) = result.source.split_once("pub task entry() {\n") else {
        return Ok(None);
    };
    let Some(text) = body
        .strip_prefix("    eval ")
        .and_then(|s| s.strip_suffix(";\n    end;\n}\n"))
    else {
        return Ok(None);
    };
    let imports = result
        .source
        .lines()
        .filter_map(|line| {
            line.strip_prefix("use std::")?
                .strip_suffix(';')
                .map(str::to_owned)
        })
        .collect();
    Ok(Some(MessageExpression {
        text: text.into(),
        imports,
        identities: result.identities,
    }))
}
