use crate::{Diagnostic, Location, ScriptKind};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Word(String),
    Number(String),
    Text(String),
    Symbol(String),
    Comment(String),
    End,
}
#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: Kind,
    pub at: Location,
}
impl Token {
    pub fn is(&self, text: &str) -> bool {
        matches!(&self.kind, Kind::Word(v) | Kind::Symbol(v) if v == text)
    }
}

pub(crate) fn lex(file: &str, source: &str) -> Result<Vec<Token>, Diagnostic> {
    let mut result = Vec::new();
    let (mut offset, mut line, mut column) = (0, 1, 1);
    while offset < source.len() {
        let rest = &source[offset..];
        let ch = rest.chars().next().unwrap();
        if ch.is_ascii_whitespace() {
            offset += ch.len_utf8();
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            continue;
        }
        let at = Location {
            file: file.into(),
            line,
            column,
        };
        let (kind, length) = if rest.starts_with("//") {
            let length = rest.find('\n').unwrap_or(rest.len());
            (Kind::Comment(rest[..length].into()), length)
        } else if rest.starts_with("/*") {
            let (mut nesting, mut length) = (1, 2);
            while nesting != 0 {
                let tail = &rest[length..];
                if tail.is_empty() {
                    return Err(at.error("unterminated block comment"));
                }
                if tail.starts_with("/*") {
                    nesting += 1;
                    length += 2;
                } else if tail.starts_with("*/") {
                    nesting -= 1;
                    length += 2;
                } else {
                    length += tail.chars().next().unwrap().len_utf8();
                }
            }
            (Kind::Comment(rest[..length].into()), length)
        } else if ch == '"' {
            let mut text = String::new();
            let mut chars = rest.char_indices().skip(1);
            let length = loop {
                let Some((position, c)) = chars.next() else {
                    return Err(at.error("unterminated text literal"));
                };
                match c {
                    '"' => break position + 1,
                    '\n' | '\r' => return Err(at.error("use \\n inside a text literal")),
                    '\\' => text.push(match chars.next().map(|(_, c)| c) {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('"') => '"',
                        Some('\\') => '\\',
                        _ => return Err(at.error("unsupported text escape")),
                    }),
                    other => text.push(other),
                }
            };
            (Kind::Text(text), length)
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let length = rest
                .bytes()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                .count();
            (Kind::Word(rest[..length].into()), length)
        } else if ch.is_ascii_digit() {
            let mut length = 0;
            let mut dot = false;
            for (position, c) in rest.char_indices() {
                if c == '.' && !dot && !rest[position..].starts_with("..") {
                    dot = true;
                } else if !c.is_ascii_alphanumeric() && c != '_' {
                    break;
                }
                length = position + c.len_utf8();
            }
            (Kind::Number(rest[..length].into()), length)
        } else if let Some(symbol) = [
            "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "..", "+=", "-=",
            "*=", "/=",
        ]
        .into_iter()
        .find(|v| rest.starts_with(v))
        {
            (Kind::Symbol(symbol.into()), symbol.len())
        } else if "{}[](),;:.=+-*/%!<>|&^".contains(ch) {
            (Kind::Symbol(ch.to_string()), 1)
        } else {
            return Err(at.error("expected ASCII identifier or punctuation"));
        };
        for c in rest[..length].chars() {
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        offset += length;
        result.push(Token { kind, at });
    }
    result.push(Token {
        kind: Kind::End,
        at: Location {
            file: file.into(),
            line,
            column,
        },
    });
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TypeRef {
    Named(String),
    Array(Box<TypeRef>, u16),
    Optional(Box<TypeRef>),
    Task(Option<Box<TypeRef>>),
}
#[derive(Clone, Debug)]
pub(crate) struct Expr {
    pub kind: Expression,
    pub at: Location,
}
#[derive(Clone, Debug)]
pub(crate) enum Expression {
    Number(String),
    Text(String),
    Name(String),
    Bool(bool),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Await(Box<Expr>),
    Spawn(String, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, String),
    Array(Vec<Expr>),
    Record(String, Vec<(String, Expr)>),
}
#[derive(Clone, Debug)]
pub(crate) struct Statement {
    pub kind: StatementKind,
    pub at: Location,
}
#[derive(Clone, Debug)]
pub(crate) enum StatementKind {
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeRef>,
        value: Expr,
    },
    Assign(Expr, String, Expr),
    Expression(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Vec<Statement>),
    While(Expr, Vec<Statement>),
    For {
        name: String,
        start: Expr,
        end: Option<Expr>,
        body: Vec<Statement>,
    },
    Match(Expr, Vec<(Pattern, Vec<Statement>)>),
    Block(Vec<Statement>),
    Defer(Vec<Statement>),
    Break,
    Continue,
}
#[derive(Clone, Debug)]
pub(crate) enum Pattern {
    Wildcard,
    Bool(bool),
    Integer(String),
    Variant(String, Vec<String>),
}
#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub name: String,
    pub public: bool,
    pub task: bool,
    pub at: Location,
    pub parameters: Vec<(String, TypeRef)>,
    pub result: Option<TypeRef>,
    pub body: Vec<Statement>,
}
#[derive(Clone, Debug)]
pub(crate) struct Binding {
    pub name: String,
    pub public: bool,
    pub asset: bool,
    pub ty: Option<TypeRef>,
    pub value: Expr,
}
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub name: String,
    pub public: bool,
    pub fields: Vec<(String, TypeRef)>,
    pub at: Location,
}
#[derive(Clone, Debug)]
pub(crate) struct Enumeration {
    pub name: String,
    pub public: bool,
    pub variants: Vec<(String, Vec<TypeRef>)>,
    pub at: Location,
}
#[derive(Clone, Debug)]
pub(crate) struct Message {
    pub name: String,
    pub public: bool,
    pub parameters: Vec<(String, TypeRef)>,
    pub text: String,
    pub at: Location,
}
#[derive(Clone, Debug)]
pub(crate) struct Module {
    pub kind: ScriptKind,
    pub imports: Vec<(String, Location)>,
    pub bindings: Vec<Binding>,
    pub functions: Vec<Function>,
    pub records: Vec<Record>,
    pub enums: Vec<Enumeration>,
    pub messages: Vec<Message>,
}

pub(crate) fn parse(file: &str, source: &str) -> Result<Module, Diagnostic> {
    parser(file, source)?.module()
}

fn parser(file: &str, source: &str) -> Result<Parser, Diagnostic> {
    let tokens = lex(file, source)?
        .into_iter()
        .filter(|t| !matches!(t.kind, Kind::Comment(_)))
        .collect();
    Ok(Parser { tokens, index: 0 })
}
struct Parser {
    tokens: Vec<Token>,
    index: usize,
}
impl Parser {
    fn header(&mut self) -> Result<ScriptKind, Diagnostic> {
        if !self.take("script") {
            return Err(self
                .token()
                .at
                .error("expected 'script field;', 'script model;', 'script battle;' or 'script library;'; script declaration must precede imports and definitions"));
        }
        let kind = match self.name()?.as_str() {
            "field" => ScriptKind::Field,
            "model" => ScriptKind::Model,
            "battle" => ScriptKind::Battle,
            "library" => ScriptKind::Library,
            _ => {
                return Err(self.tokens[self.index - 1]
                    .at
                    .error("unknown script kind; expected field, model or library"));
            }
        };
        self.expect(";")?;
        Ok(kind)
    }
    fn token(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn take(&mut self, value: &str) -> bool {
        if self.token().is(value) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, value: &str) -> Result<(), Diagnostic> {
        if value == ">" && self.token().is(">>") {
            self.tokens[self.index].kind = Kind::Symbol(">".into());
            return Ok(());
        }
        if self.take(value) {
            Ok(())
        } else {
            Err(self.token().at.error(format!("expected '{value}'")))
        }
    }
    fn name(&mut self) -> Result<String, Diagnostic> {
        if let Kind::Word(value) = self.token().kind.clone() {
            self.index += 1;
            Ok(value)
        } else {
            Err(self.token().at.error("expected identifier"))
        }
    }
    fn path(&mut self) -> Result<String, Diagnostic> {
        let mut result = self.name()?;
        while self.take("::") {
            result.push_str("::");
            result.push_str(&self.name()?);
        }
        Ok(result)
    }
    fn ty(&mut self) -> Result<TypeRef, Diagnostic> {
        if self.take("[") {
            let item = self.ty()?;
            self.expect(";")?;
            let Kind::Number(length) = self.token().kind.clone() else {
                return Err(self.token().at.error("expected fixed array length"));
            };
            let length = length
                .parse::<u16>()
                .map_err(|_| self.token().at.error("array length must fit u16"))?;
            self.index += 1;
            self.expect("]")?;
            return Ok(TypeRef::Array(Box::new(item), length));
        }
        let name = self.path()?;
        if name == "Option" && self.take("<") {
            let inner = self.ty()?;
            self.expect(">")?;
            Ok(TypeRef::Optional(Box::new(inner)))
        } else if name == "Task" {
            let result = if self.take("<") {
                let result = self.ty()?;
                self.expect(">")?;
                Some(Box::new(result))
            } else {
                None
            };
            Ok(TypeRef::Task(result))
        } else {
            Ok(TypeRef::Named(name))
        }
    }
    fn module(mut self) -> Result<Module, Diagnostic> {
        let mut module = Module {
            kind: self.header()?,
            imports: Vec::new(),
            bindings: Vec::new(),
            functions: Vec::new(),
            records: Vec::new(),
            enums: Vec::new(),
            messages: Vec::new(),
        };
        while !matches!(self.token().kind, Kind::End) {
            let at = self.token().at.clone();
            if self.token().is("script") {
                return Err(
                    at.error("script kind may only be declared once, at the top of the file")
                );
            }
            let public = self.take("pub");
            if self.take("use") {
                if public {
                    return Err(at.error("imports cannot be public"));
                }
                module.imports.push((self.path()?, at));
                self.expect(";")?;
            } else if self.token().is("const")
                || self.token().is("asset")
                || self.token().is("message")
            {
                let asset = self.take("asset");
                let message = !asset && self.take("message");
                if !asset && !message {
                    self.expect("const")?;
                }
                let name = self.name()?;
                if message && self.take("(") {
                    let mut parameters = Vec::new();
                    while !self.take(")") {
                        let parameter = self.name()?;
                        self.expect(":")?;
                        parameters.push((parameter, self.ty()?));
                        if !self.take(",") {
                            self.expect(")")?;
                            break;
                        }
                    }
                    self.expect("=")?;
                    let Kind::Text(text) = self.token().kind.clone() else {
                        return Err(self
                            .token()
                            .at
                            .error("message template requires a text literal"));
                    };
                    self.index += 1;
                    self.expect(";")?;
                    module.messages.push(Message {
                        name,
                        public,
                        parameters,
                        text,
                        at,
                    });
                    continue;
                }
                let ty = if self.take(":") {
                    Some(self.ty()?)
                } else if message {
                    Some(TypeRef::Named("Message".into()))
                } else {
                    None
                };
                if asset && ty.is_none() {
                    return Err(at.error("asset declarations require a type"));
                }
                self.expect("=")?;
                let value = self.expr(0)?;
                self.expect(";")?;
                module.bindings.push(Binding {
                    name,
                    public,
                    asset,
                    ty,
                    value,
                });
            } else if self.take("struct") {
                let name = self.name()?;
                self.expect("{")?;
                let mut fields = Vec::new();
                while !self.take("}") {
                    let name = self.name()?;
                    self.expect(":")?;
                    fields.push((name, self.ty()?));
                    if !self.take(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                module.records.push(Record {
                    name,
                    public,
                    fields,
                    at,
                });
            } else if self.take("enum") {
                let name = self.name()?;
                self.expect("{")?;
                let mut variants = Vec::new();
                while !self.take("}") {
                    let variant = self.name()?;
                    let mut payload = Vec::new();
                    if self.take("(") {
                        while !self.take(")") {
                            payload.push(self.ty()?);
                            if !self.take(",") {
                                self.expect(")")?;
                                break;
                            }
                        }
                    }
                    variants.push((variant, payload));
                    if !self.take(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                module.enums.push(Enumeration {
                    name,
                    public,
                    variants,
                    at,
                });
            } else {
                let task = self.take("task");
                if !task {
                    self.expect("fn")?;
                }
                let name = self.name()?;
                self.expect("(")?;
                let mut parameters = Vec::new();
                while !self.take(")") {
                    let name = self.name()?;
                    self.expect(":")?;
                    parameters.push((name, self.ty()?));
                    if !self.take(",") {
                        self.expect(")")?;
                        break;
                    }
                }
                let result = if self.take("->") {
                    Some(self.ty()?)
                } else {
                    None
                };
                let body = self.block()?;
                module.functions.push(Function {
                    name,
                    public,
                    task,
                    at,
                    parameters,
                    result,
                    body,
                });
            }
        }
        Ok(module)
    }
    fn block(&mut self) -> Result<Vec<Statement>, Diagnostic> {
        self.expect("{")?;
        let mut statements = Vec::new();
        while !self.take("}") {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }
    fn statement(&mut self) -> Result<Statement, Diagnostic> {
        let at = self.token().at.clone();
        let kind = if self.take("defer") {
            StatementKind::Defer(self.block()?)
        } else if self.token().is("{") {
            StatementKind::Block(self.block()?)
        } else if self.take("let") {
            let mutable = self.take("mut");
            let name = self.name()?;
            let ty = if self.take(":") {
                Some(self.ty()?)
            } else {
                None
            };
            self.expect("=")?;
            let value = self.expr(0)?;
            self.expect(";")?;
            StatementKind::Let {
                name,
                mutable,
                ty,
                value,
            }
        } else if self.take("return") {
            let value = if self.token().is(";") {
                None
            } else {
                Some(self.expr(0)?)
            };
            self.expect(";")?;
            StatementKind::Return(value)
        } else if self.take("if") {
            let test = self.expr(0)?;
            let yes = self.block()?;
            let no = if self.take("else") {
                if self.token().is("if") {
                    vec![self.statement()?]
                } else {
                    self.block()?
                }
            } else {
                Vec::new()
            };
            StatementKind::If(test, yes, no)
        } else if self.take("while") {
            let test = self.expr(0)?;
            StatementKind::While(test, self.block()?)
        } else if self.take("for") {
            let name = self.name()?;
            self.expect("in")?;
            let start = self.expr(0)?;
            let end = if self.take("..") {
                Some(self.expr(0)?)
            } else {
                None
            };
            StatementKind::For {
                name,
                start,
                end,
                body: self.block()?,
            }
        } else if self.take("match") {
            let value = self.expr(0)?;
            self.expect("{")?;
            let mut arms = Vec::new();
            while !self.take("}") {
                let pattern = if self.take("_") {
                    Pattern::Wildcard
                } else if self.take("true") {
                    Pattern::Bool(true)
                } else if self.take("false") {
                    Pattern::Bool(false)
                } else if self.take("-") {
                    let Kind::Number(number) = self.token().kind.clone() else {
                        return Err(self.token().at.error("expected integer pattern"));
                    };
                    self.index += 1;
                    Pattern::Integer(format!("-{number}"))
                } else if let Kind::Number(number) = self.token().kind.clone() {
                    self.index += 1;
                    Pattern::Integer(number)
                } else {
                    let name = self.path()?;
                    let mut bindings = Vec::new();
                    if self.take("(") {
                        while !self.take(")") {
                            bindings.push(self.name()?);
                            if !self.take(",") {
                                self.expect(")")?;
                                break;
                            }
                        }
                    }
                    Pattern::Variant(name, bindings)
                };
                self.expect("=>")?;
                arms.push((pattern, self.block()?));
                if !self.take(",") {
                    self.expect("}")?;
                    break;
                }
            }
            StatementKind::Match(value, arms)
        } else if self.take("break") {
            self.expect(";")?;
            StatementKind::Break
        } else if self.take("continue") {
            self.expect(";")?;
            StatementKind::Continue
        } else {
            let expr = self.expr(0)?;
            if let Some(op) = ["=", "+=", "-=", "*=", "/="]
                .into_iter()
                .find(|op| self.token().is(op))
            {
                self.index += 1;
                let value = self.expr(0)?;
                self.expect(";")?;
                StatementKind::Assign(expr, op.into(), value)
            } else {
                self.expect(";")?;
                StatementKind::Expression(expr)
            }
        };
        Ok(Statement { kind, at })
    }
    fn expr(&mut self, minimum: u8) -> Result<Expr, Diagnostic> {
        let at = self.token().at.clone();
        let mut left = if self.token().is("-") || self.token().is("!") {
            let Kind::Symbol(op) = self.token().kind.clone() else {
                unreachable!()
            };
            self.index += 1;
            Expr {
                kind: Expression::Unary(op, Box::new(self.expr(12)?)),
                at,
            }
        } else if self.take("await") {
            Expr {
                kind: Expression::Await(Box::new(self.expr(12)?)),
                at,
            }
        } else if self.take("spawn") {
            let name = self.path()?;
            let args = self.arguments()?;
            Expr {
                kind: Expression::Spawn(name, args),
                at,
            }
        } else if self.take("(") {
            let expr = self.expr(0)?;
            self.expect(")")?;
            expr
        } else if self.take("[") {
            let mut values = Vec::new();
            while !self.take("]") {
                values.push(self.expr(0)?);
                if !self.take(",") {
                    self.expect("]")?;
                    break;
                }
            }
            Expr {
                kind: Expression::Array(values),
                at,
            }
        } else {
            let token = self.token().clone();
            match token.kind {
                Kind::Number(value) => {
                    self.index += 1;
                    Expr {
                        kind: Expression::Number(value),
                        at,
                    }
                }
                Kind::Text(value) => {
                    self.index += 1;
                    Expr {
                        kind: Expression::Text(value),
                        at,
                    }
                }
                Kind::Word(_) => {
                    let name = self.path()?;
                    let kind = if name == "true" || name == "false" {
                        Expression::Bool(name == "true")
                    } else if self.token().is("(") {
                        Expression::Call(name, self.arguments()?)
                    } else if self.token().is("{")
                        && self.tokens.get(self.index + 2).is_some_and(|t| t.is(":"))
                    {
                        self.index += 1;
                        let mut fields = Vec::new();
                        while !self.take("}") {
                            let field = self.name()?;
                            self.expect(":")?;
                            fields.push((field, self.expr(0)?));
                            if !self.take(",") {
                                self.expect("}")?;
                                break;
                            }
                        }
                        Expression::Record(name, fields)
                    } else {
                        Expression::Name(name)
                    };
                    Expr { kind, at }
                }
                _ => return Err(at.error("expected expression")),
            }
        };
        loop {
            if self.take("[") {
                let index = self.expr(0)?;
                self.expect("]")?;
                left = Expr {
                    at: left.at.clone(),
                    kind: Expression::Index(Box::new(left), Box::new(index)),
                };
                continue;
            }
            if self.take(".") {
                left = Expr {
                    at: left.at.clone(),
                    kind: Expression::Field(Box::new(left), self.name()?),
                };
                continue;
            }
            let Some((op, precedence)) = binary(&self.token().kind) else {
                break;
            };
            if precedence < minimum {
                break;
            }
            let op = op.to_owned();
            self.index += 1;
            let right = self.expr(precedence + 1)?;
            left = Expr {
                at: left.at.clone(),
                kind: Expression::Binary(op, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }
    fn arguments(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        self.expect("(")?;
        let mut arguments = Vec::new();
        while !self.take(")") {
            arguments.push(self.expr(0)?);
            if !self.take(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(arguments)
    }
}
fn binary(kind: &Kind) -> Option<(&str, u8)> {
    let Kind::Symbol(op) = kind else {
        return None;
    };
    Some((
        op,
        match op.as_str() {
            "||" => 1,
            "&&" => 2,
            "|" => 3,
            "^" => 4,
            "&" => 5,
            "==" | "!=" => 6,
            "<" | "<=" | ">" | ">=" => 7,
            "<<" | ">>" => 8,
            "+" | "-" => 9,
            "*" | "/" | "%" => 10,
            _ => return None,
        },
    ))
}
