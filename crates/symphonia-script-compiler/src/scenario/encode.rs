use super::{Definitions, catalog::number, literal_width};
use crate::{
    Diagnostic,
    syntax::{self, Kind, Token},
};
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script::{
    Program, scenario,
    semantics::{self, NativeRegistry},
};

/// Compile readable original-VM source into a self-contained scenario resource.
/// Labels relocate branches, event PCs and auxiliary offsets after source edits.
pub fn compile(file: &str, source: &str, definitions: &Definitions) -> Result<Vec<u8>, Diagnostic> {
    compile_program(file, source, definitions).map(|(bytes, _)| bytes)
}

/// Retain the validated program when preparing source for execution.
pub fn compile_program(
    file: &str,
    source: &str,
    definitions: &Definitions,
) -> Result<(Vec<u8>, Program), Diagnostic> {
    let parser = parser(file, source, definitions, false)?;
    let at = parser.tokens.last().unwrap().at.clone();
    let bytes = parser.module()?;
    let program = Program::decode(&bytes).map_err(|e| at.error(e.to_string()))?;
    Ok((bytes, program))
}

pub(crate) fn validate_syntax(file: &str, source: &str) -> Result<(), Diagnostic> {
    parser(file, source, &Definitions::default(), true)?
        .module()
        .map(|_| ())
}

fn parser<'a>(
    file: &str,
    source: &str,
    definitions: &'a Definitions,
    syntax_only: bool,
) -> Result<Parser<'a>, Diagnostic> {
    let tokens = syntax::lex(file, source)?
        .into_iter()
        .filter(|t| !matches!(t.kind, Kind::Comment(_)))
        .collect();
    Ok(Parser {
        tokens,
        index: 0,
        definitions,
        registry: NativeRegistry::gqseaf(),
        syntax_only,
        assembly: String::new(),
        offset: 0,
        code_base: None,
        labels: BTreeMap::new(),
        fixups: Vec::new(),
        imports: BTreeSet::new(),
        temporary: 0,
        message_expressions: Vec::new(),
    })
}

struct Parser<'a> {
    tokens: Vec<Token>,
    index: usize,
    definitions: &'a Definitions,
    registry: NativeRegistry,
    assembly: String,
    offset: usize,
    code_base: Option<u16>,
    labels: BTreeMap<String, usize>,
    fixups: Vec<(usize, String, Fixup)>,
    imports: BTreeSet<String>,
    temporary: u32,
    syntax_only: bool,
    message_expressions: Vec<(usize, usize)>,
}
#[derive(Clone)]
enum Fixup {
    Pc16,
    Pc32,
    Offset,
    Relative16(String),
    Relative32(String),
}

impl Parser<'_> {
    fn token(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn take(&mut self, s: &str) -> bool {
        if self.token().is(s) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> Result<(), Diagnostic> {
        if self.take(s) {
            Ok(())
        } else {
            Err(self.token().at.error(format!("expected '{s}'")))
        }
    }
    fn name(&mut self) -> Result<String, Diagnostic> {
        if let Kind::Word(s) = self.token().kind.clone() {
            self.index += 1;
            Ok(s)
        } else {
            Err(self.token().at.error("expected identifier"))
        }
    }
    fn path(&mut self) -> Result<String, Diagnostic> {
        let mut s = self.name()?;
        while self.take("::") {
            s.push_str("::");
            s.push_str(&self.name()?);
        }
        Ok(s)
    }
    fn integer(&mut self) -> Result<i64, Diagnostic> {
        let negative = self.take("-");
        let at = self.token().at.clone();
        let value = if self.take("flags") {
            self.expect("(")?;
            let mut value = self.integer()?;
            while self.take(",") {
                value |= self.integer()?;
            }
            self.expect(")")?;
            value
        } else if self.take("bound_before") {
            self.expect("(")?;
            let anchor = i32::try_from(self.integer()?)
                .map_err(|_| at.error("bound_before anchor exceeds i32"))?;
            self.expect(")")?;
            i64::from(
                anchor
                    .checked_sub(1)
                    .ok_or_else(|| at.error("bound_before result is below i32::MIN"))?,
            )
        } else if self.take("voice_archive") {
            self.expect("(")?;
            let base = self.integer()?;
            self.expect(",")?;
            let member = self.integer()?;
            self.expect(")")?;
            if !(0..=i64::from(u32::MAX)).contains(&base) || base & 0xffff != 0 {
                return Err(at.error("voice_archive requires an aligned u32 archive base"));
            }
            if !(0..=65535).contains(&member) {
                return Err(at.error("voice_archive member must fit u16"));
            }
            i64::from((base as u32 | member as u32) as i32)
        } else if self.take("flag_slot") {
            self.expect("(")?;
            let base = self.integer()?;
            self.expect(",")?;
            let slot = self.integer()?;
            self.expect(")")?;
            let value = base
                .checked_add(slot)
                .ok_or_else(|| at.error("flag_slot result exceeds i32"))?;
            if base < 0 || slot < 0 || value > i64::from(i32::MAX) {
                return Err(at.error("flag_slot requires nonnegative constants whose sum fits i32"));
            }
            value
        } else if let Kind::Number(n) = self.token().kind.clone() {
            self.index += 1;
            number(&n).ok_or_else(|| at.error("expected integer"))?
        } else {
            let path = self.path()?;
            let domain = path
                .strip_prefix("std::")
                .unwrap_or(&path)
                .split("::")
                .next()
                .unwrap();
            if !self.imports.contains(domain) {
                return Err(at.error(format!("constant '{path}' requires 'use std::{domain};'")));
            }
            i64::from(
                self.definitions
                    .value(&path)
                    .or(self.syntax_only.then_some(0))
                    .ok_or_else(|| at.error(format!("unknown constant '{path}'")))?,
            )
        };
        if negative {
            value
                .checked_neg()
                .ok_or_else(|| at.error("integer out of range"))
        } else {
            Ok(value)
        }
    }
    fn emit(&mut self, mnemonic: &str, operand: impl std::fmt::Display) {
        use std::fmt::Write;
        writeln!(self.assembly, "{mnemonic} {operand}").unwrap();
        self.offset += match mnemonic {
            "push.s32" => 6,
            "push.s16" | "push.u16" | "load.s8" | "load.s16" | "load.s32" | "jump" | "call"
            | "branch_false" => 4,
            ".byte" => 1,
            _ => 2,
        };
    }
    fn label(&mut self, name: String) -> Result<(), Diagnostic> {
        if self.labels.insert(name.clone(), self.offset).is_some() {
            return Err(self.token().at.error(format!("duplicate label '{name}'")));
        }
        Ok(())
    }
    fn reference(&mut self, label: String, kind: Fixup) {
        let wide = matches!(kind, Fixup::Pc32 | Fixup::Relative32(_));
        self.fixups.push((self.offset, label, kind));
        self.emit(".word", 0);
        if wide {
            self.emit(".word", 0);
        }
    }
    fn branch(&mut self, mnemonic: &str, label: String) {
        self.emit(
            ".word",
            match mnemonic {
                "jump" => 0x2001,
                "call" => 0x2002,
                _ => 0x2004,
            },
        );
        self.reference(label, Fixup::Pc16);
    }
    fn temporary(&mut self) -> String {
        self.temporary += 1;
        format!("$flow{}", self.temporary)
    }
    fn dword(&mut self, value: u32) {
        self.emit(".word", value >> 16);
        self.emit(".word", value & 0xffff);
    }
    fn events(&mut self) -> Result<(), Diagnostic> {
        let at = self.token().at.clone();
        if self.offset != 0 || self.code_base.is_some() || !self.labels.is_empty() {
            return Err(at.error("events must precede resource data and replace code_base"));
        }
        self.expect("{")?;
        let mut entry = None;
        let mut auxiliary = None;
        let mut records = Vec::new();
        while !self.take("}") {
            if self.take("entry") {
                if entry.replace(self.name()?).is_some() {
                    return Err(at.error("duplicate event entry"));
                }
            } else if self.take("auxiliary") {
                if auxiliary.replace(self.name()?).is_some() {
                    return Err(at.error("duplicate auxiliary table"));
                }
            } else {
                self.expect("on")?;
                let kind = u32::try_from(self.integer()?)
                    .map_err(|_| self.token().at.error("event kind must fit u32"))?;
                self.expect("(")?;
                let key = u32::try_from(self.integer()?)
                    .map_err(|_| self.token().at.error("event key must fit u32"))?;
                self.expect(")")?;
                self.expect("=>")?;
                records.push((kind, key, self.name()?));
            }
            self.expect(";")?;
        }
        let entry = entry.ok_or_else(|| at.error("events requires an entry label"))?;
        let base = u16::try_from(4 + records.len() * 6)
            .map_err(|_| at.error("event registry exceeds the code base limit"))?;
        self.code_base = Some(base);
        self.emit(".word", base);
        self.reference(entry, Fixup::Pc16);
        if let Some(auxiliary) = auxiliary {
            self.reference(auxiliary, Fixup::Offset);
        } else {
            self.emit(".word", 0);
        }
        self.emit(".word", records.len());
        // Order and duplicate keys are significant: native lookup takes the
        // first matching record, even when later records name other tasks.
        for (kind, key, task) in records {
            self.dword(kind);
            self.dword(key);
            self.reference(task, Fixup::Pc32);
        }
        Ok(())
    }
    fn module(mut self) -> Result<Vec<u8>, Diagnostic> {
        self.expect("script")?;
        self.expect("scenario")?;
        self.expect(";")?;
        while !matches!(self.token().kind, Kind::End) {
            if self.take("use") {
                let path = self.path()?;
                let Some(module) = path.strip_prefix("std::").filter(|m| !m.contains("::")) else {
                    return Err(self
                        .token()
                        .at
                        .error("scenario imports must name a generated std module"));
                };
                self.imports.insert(module.into());
                self.expect(";")?;
            } else if self.take("events") {
                self.events()?;
            } else if self.take("code_base") {
                self.expect("=")?;
                let n = self.integer()?;
                let base = u16::try_from(n)
                    .map_err(|_| self.token().at.error("code_base must fit u16"))?;
                if self.code_base.replace(base).is_some() {
                    return Err(self.token().at.error("duplicate code_base"));
                }
                self.expect(";")?;
            } else if self.take("data") {
                self.data()?;
            } else {
                let public = self.take("pub");
                if self.take("task") {
                    let name = self.name()?;
                    self.label(name)?;
                    self.expect("(")?;
                    self.expect(")")?;
                    self.block()?;
                } else if !public {
                    let label = self.name()?;
                    self.expect(":")?;
                    self.label(label)?;
                } else {
                    return Err(self.token().at.error("expected task"));
                }
            }
        }
        let base = self
            .code_base
            .ok_or_else(|| self.token().at.error("missing code_base"))?;
        let mut bytes = scenario::assemble(&format!(".code_base {base}\n{}", self.assembly))
            .map_err(|e| self.token().at.error(e.to_string()))?;
        for (at, name, kind) in &self.fixups {
            let offset = *self
                .labels
                .get(name)
                .ok_or_else(|| self.token().at.error(format!("undefined label '{name}'")))?;
            if let Fixup::Relative16(base) | Fixup::Relative32(base) = kind {
                let base_offset = *self
                    .labels
                    .get(base)
                    .ok_or_else(|| self.token().at.error(format!("undefined label '{base}'")))?;
                let relative = offset.checked_sub(base_offset).ok_or_else(|| {
                    self.token()
                        .at
                        .error(format!("relative target '{name}' precedes base '{base}'"))
                })?;
                if matches!(kind, Fixup::Relative32(_)) {
                    let n = u32::try_from(relative)
                        .map_err(|_| self.token().at.error("relative byte offset exceeds u32"))?;
                    bytes[*at..*at + 4].copy_from_slice(&n.to_be_bytes());
                } else {
                    let n = u16::try_from(relative)
                        .map_err(|_| self.token().at.error("relative byte offset exceeds u16"))?;
                    bytes[*at..*at + 2].copy_from_slice(&n.to_be_bytes());
                }
                continue;
            }
            let n = match kind {
                Fixup::Offset => offset,
                _ => offset.checked_sub(usize::from(base) * 2).ok_or_else(|| {
                    self.token()
                        .at
                        .error(format!("label '{name}' precedes code base"))
                })?,
            };
            if n % 2 != 0 {
                return Err(self
                    .token()
                    .at
                    .error(format!("label '{name}' is not word aligned")));
            }
            let n = n / 2;
            if matches!(kind, Fixup::Pc32) {
                let n = u32::try_from(n).map_err(|_| self.token().at.error("PC exceeds u32"))?;
                bytes[*at..*at + 4].copy_from_slice(&n.to_be_bytes());
            } else {
                let n = u16::try_from(n)
                    .map_err(|_| self.token().at.error("PC or offset exceeds u16"))?;
                bytes[*at..*at + 2].copy_from_slice(&n.to_be_bytes());
            }
        }
        if !self.syntax_only {
            for &(start, end) in &self.message_expressions {
                symphonia_script::message::Expression::from_native(&bytes[start..end]).map_err(
                    |e| {
                        self.token()
                            .at
                            .error(format!("invalid message control expression: {e}"))
                    },
                )?;
            }
        }
        Ok(bytes)
    }
    fn data(&mut self) -> Result<(), Diagnostic> {
        self.expect("{")?;
        while !self.take("}") {
            let kind = self.name()?;
            self.expect("(")?;
            match kind.as_str() {
                "pc" | "pc32" | "offset" => {
                    let label = self.name()?;
                    self.reference(
                        label,
                        match kind.as_str() {
                            "pc" => Fixup::Pc16,
                            "pc32" => Fixup::Pc32,
                            _ => Fixup::Offset,
                        },
                    );
                }
                "relative16" | "relative32" => {
                    let label = self.name()?;
                    self.expect(",")?;
                    let base = self.name()?;
                    self.reference(
                        label,
                        if kind == "relative16" {
                            Fixup::Relative16(base)
                        } else {
                            Fixup::Relative32(base)
                        },
                    );
                }
                "message_control" => {
                    let opcode = u8::try_from(self.integer()?)
                        .map_err(|_| self.token().at.error("message control must fit u8"))?;
                    if !self.syntax_only && !matches!(opcode, 1..=9 | 17 | 18) {
                        return Err(self.token().at.error("unrecognized message control"));
                    }
                    self.expect(",")?;
                    self.emit(".byte", opcode);
                    let start = self.offset;
                    self.expression(0)?;
                    self.emit("calc", 0);
                    self.emit("end", "");
                    self.message_expressions.push((start, self.offset));
                }
                "sjis" => {
                    let Kind::Text(text) = self.token().kind.clone() else {
                        return Err(self.token().at.error("expected Shift-JIS text literal"));
                    };
                    let (bytes, _, errors) = encoding_rs::SHIFT_JIS.encode(&text);
                    if errors {
                        return Err(self
                            .token()
                            .at
                            .error("text cannot be represented in Shift-JIS"));
                    }
                    self.index += 1;
                    for byte in bytes.iter() {
                        self.emit(".byte", byte);
                    }
                }
                "word" | "byte" => {
                    let n = self.integer()?;
                    self.emit(if kind == "word" { ".word" } else { ".byte" }, n);
                }
                "dword" => {
                    let n = u32::try_from(self.integer()?)
                        .map_err(|_| self.token().at.error("dword must fit u32"))?;
                    self.dword(n);
                }
                "hex" => {
                    let Kind::Text(text) = self.token().kind.clone() else {
                        return Err(self.token().at.error("expected hex string"));
                    };
                    self.index += 1;
                    let text: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                    if !text.len().is_multiple_of(2) || !text.is_ascii() {
                        return Err(self
                            .token()
                            .at
                            .error("hex data requires pairs of ASCII digits"));
                    }
                    for pair in text.as_bytes().chunks(2) {
                        let n = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                            .map_err(|_| self.token().at.error("invalid hex byte"))?;
                        self.emit(".byte", n);
                    }
                }
                _ => {
                    return Err(self
                        .token()
                        .at
                        .error("expected word, dword, byte, pc, pc32, offset, relative16, relative32, sjis, message_control or hex"));
                }
            }
            self.expect(")")?;
            self.expect(";")?;
        }
        Ok(())
    }
    fn block(&mut self) -> Result<(), Diagnostic> {
        self.expect("{")?;
        while !self.take("}") {
            if matches!(self.token().kind, Kind::End) {
                return Err(self.token().at.error("unclosed block"));
            }
            self.statement()?;
        }
        Ok(())
    }
    fn evaluate(&mut self) -> Result<(), Diagnostic> {
        if !self.take("result") {
            self.expression(0)?;
            self.emit("calc", 0);
        }
        Ok(())
    }
    fn statement(&mut self) -> Result<(), Diagnostic> {
        if self.tokens.get(self.index + 1).is_some_and(|t| t.is(":")) {
            let name = self.name()?;
            self.expect(":")?;
            return self.label(name);
        }
        if self.take("if_false") {
            self.expect("(")?;
            self.evaluate()?;
            self.expect(")")?;
            self.expect("goto")?;
            let label = self.name()?;
            self.branch("branch_false", label);
        } else if self.take("if") {
            self.expect("(")?;
            self.evaluate()?;
            self.expect(")")?;
            let otherwise = self.temporary();
            self.branch("branch_false", otherwise.clone());
            self.block()?;
            if self.take("else") {
                let end = self.temporary();
                self.branch("jump", end.clone());
                self.label(otherwise)?;
                self.block()?;
                self.label(end)?;
            } else {
                self.label(otherwise)?;
            }
            return Ok(());
        } else if self.take("while") {
            let start = self.temporary();
            let end = self.temporary();
            self.label(start.clone())?;
            self.expect("(")?;
            self.evaluate()?;
            self.expect(")")?;
            self.branch("branch_false", end.clone());
            self.block()?;
            self.branch("jump", start);
            self.label(end)?;
            return Ok(());
        } else if self.take("goto") {
            let name = self.name()?;
            self.branch("jump", name);
        } else if self.take("call") {
            let name = self.name()?;
            self.branch("call", name);
        } else if self.take("return") {
            self.emit("ret", "");
        } else if self.take("end") {
            self.emit("end", "");
        } else if self.take("push") {
            self.expression(0)?;
        } else if self.take("argument") {
            self.evaluate()?;
            self.emit("arg", "");
        } else if self.take("eval") {
            self.evaluate()?;
        } else if self.take("vm") {
            self.expect("::")?;
            let op = self.name()?;
            self.expect("(")?;
            match op.as_str() {
                "calculate" => {
                    let n = if matches!(self.token().kind, Kind::Word(_)) {
                        let name = self.name()?;
                        i64::from(
                            semantics::calculator_by_name(&name)
                                .ok_or_else(|| {
                                    self.token().at.error("unknown calculator operation")
                                })?
                                .opcode,
                        )
                    } else {
                        self.integer()?
                    };
                    self.emit("calc", n);
                }
                "argument" => self.emit("arg", ""),
                "native" => {
                    let opcode = if matches!(self.token().kind, Kind::Number(_)) {
                        self.integer()?
                    } else {
                        let name = self.name()?;
                        i64::from(
                            self.registry
                                .get_by_name(&name)
                                .ok_or_else(|| self.token().at.error("unknown native"))?
                                .opcode,
                        )
                    };
                    self.emit("proc", opcode);
                }
                _ => return Err(self.token().at.error("unknown VM intrinsic")),
            }
            self.expect(")")?;
        } else {
            // A native statement leaves its VM result on the operand stack. An
            // ordinary expression statement ends the calculator expression.
            let native = self.token().is("native");
            self.expression(0)?;
            if !native {
                self.emit("calc", 0);
            }
        }
        self.expect(";")
    }
    fn expression(&mut self, minimum: u8) -> Result<(), Diagnostic> {
        if self.take("(") {
            self.expression(0)?;
            self.expect(")")?;
        } else if ["++", "--", "-", "+", "~", "!"]
            .iter()
            .any(|s| self.token().is(s))
        {
            let op = self.token().kind.clone();
            self.index += 1;
            // Negative literals are single pushes, unlike -(expression).
            if matches!(&op, Kind::Symbol(s) if s == "-")
                && matches!(self.token().kind, Kind::Number(_))
            {
                let n = self
                    .integer()?
                    .checked_neg()
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or_else(|| self.token().at.error("literal exceeds i32"))?;
                self.emit(&format!("push.{}", literal_width(n)), n);
            } else {
                self.expression(13)?;
                let Kind::Symbol(op) = op else { unreachable!() };
                self.emit(
                    "calc",
                    match op.as_str() {
                        "++" => 3,
                        "--" => 4,
                        "-" => 5,
                        "+" => 6,
                        "~" => 7,
                        _ => 8,
                    },
                );
            }
        } else if self.take("stack_depth") {
            // No push: the following expression-end/argument/push sequence
            // reads the retail interpreter's aliased stack-depth word.
            self.expect("(")?;
            self.expect(")")?;
        } else if self.take("encoded") {
            let at = self.token().at.clone();
            self.expect("(")?;
            let expected = i32::try_from(self.integer()?)
                .map_err(|_| at.error("encoded identity exceeds i32"))?;
            self.expect(",")?;
            let start = self.assembly.len();
            self.expression(0)?;
            self.expect(")")?;
            let actual = super::constant::assembly(&self.assembly[start..])
                .ok_or_else(|| at.error("encoded identity requires a constant expression without calls or memory accesses"))?;
            if !self.syntax_only && expected != actual {
                return Err(at.error(format!(
                    "encoded identity is {expected}, but its expression evaluates to {actual}"
                )));
            }
        } else if self.take("globals") {
            self.expect("[")?;
            let index = self.integer()?;
            self.expect("]")?;
            let offset = index
                .checked_mul(4)
                .and_then(|i| i.checked_add(0x40))
                .filter(|&offset| (0x40..0x440).contains(&offset))
                .ok_or_else(|| self.token().at.error("global index must be in 0..256"))?;
            self.emit("load.s32", offset);
        } else if self.take("memory") {
            self.expect("::")?;
            let width = self.name()?;
            if !matches!(width.as_str(), "i8" | "i16" | "i32") {
                return Err(self.token().at.error("memory width must be i8, i16 or i32"));
            }
            self.expect("[")?;
            let offset = self.integer()?;
            self.expect("]")?;
            self.emit(&format!("load.s{}", &width[1..]), offset);
        } else if self.take("native") {
            self.expect("::")?;
            let name = self.name()?;
            let native = self
                .registry
                .get_by_name(&name)
                .ok_or_else(|| self.token().at.error(format!("unknown native '{name}'")))?
                .clone();
            if native.control_flow {
                return Err(self
                    .token()
                    .at
                    .error("control-flow opcode is not a native call"));
            }
            self.expect("(")?;
            let mut count = 0;
            if !self.take(")") {
                loop {
                    if self.tokens.get(self.index + 1).is_some_and(|t| t.is(":")) {
                        let parameter = self.name()?;
                        self.expect(":")?;
                        if !native.accepts_argument_name(count, &parameter) {
                            return Err(self
                                .token()
                                .at
                                .error(format!("unexpected parameter '{parameter}' in '{name}'")));
                        }
                    }
                    self.evaluate()?;
                    self.emit("arg", "");
                    count += 1;
                    if self.take(")") {
                        break;
                    }
                    self.expect(",")?;
                }
            }
            if count != native.arguments.len() {
                return Err(self.token().at.error(format!(
                    "'{name}' expects {} arguments, received {count}",
                    native.arguments.len()
                )));
            }
            self.emit("proc", native.opcode);
        } else if ["s8", "s16", "s32", "u16"]
            .iter()
            .any(|s| self.token().is(s))
        {
            let width = self.name()?;
            self.expect("(")?;
            let n = self.integer()?;
            self.expect(")")?;
            self.emit(&format!("push.{width}"), n);
        } else {
            let n = if self.take("true") {
                1
            } else if self.take("false") {
                0
            } else {
                i32::try_from(self.integer()?)
                    .map_err(|_| self.token().at.error("literal exceeds i32"))?
            };
            self.emit(&format!("push.{}", literal_width(n)), n);
        }
        loop {
            if minimum <= 14 && self.take("[") {
                self.expression(0)?;
                self.expect("]")?;
                self.emit("calc", 0x0f);
                continue;
            }
            if minimum <= 14 && (self.token().is("++") || self.token().is("--")) {
                let increment = self.take("++");
                if !increment {
                    self.expect("--")?;
                }
                self.emit("calc", if increment { 1 } else { 2 });
                continue;
            }
            let Some((precedence, opcode)) = binary(self.token()) else {
                break;
            };
            if precedence < minimum {
                break;
            }
            self.index += 1;
            self.expression(if precedence == 1 {
                precedence
            } else {
                precedence + 1
            })?;
            self.emit("calc", opcode);
        }
        Ok(())
    }
}

fn binary(token: &Token) -> Option<(u8, u8)> {
    let Kind::Symbol(s) = &token.kind else {
        return None;
    };
    Some(match s.as_str() {
        "=" => (1, 0x10),
        "+=" => (1, 0x11),
        "-=" => (1, 0x12),
        "*=" => (1, 0x13),
        "/=" => (1, 0x14),
        "%=" => (1, 0x15),
        "&=" => (1, 0x16),
        "|=" => (1, 0x17),
        "^=" => (1, 0x18),
        "<<=" => (1, 0x19),
        ">>=" => (1, 0x1a),
        "||" => (2, 0x27),
        "&&" => (3, 0x26),
        "|" => (4, 0x37),
        "^" => (5, 0x38),
        "&" => (6, 0x36),
        "==" => (7, 0x20),
        "!=" => (7, 0x21),
        "<=" => (8, 0x22),
        ">=" => (8, 0x23),
        ">" => (8, 0x24),
        "<" => (8, 0x25),
        "<<" => (9, 0x39),
        ">>" => (9, 0x3a),
        "+" => (10, 0x31),
        "-" => (10, 0x32),
        "*" => (11, 0x33),
        "/" => (11, 0x34),
        "%" => (11, 0x35),
        _ => return None,
    })
}
