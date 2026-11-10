use crate::{Diagnostic, ScriptKind, SourceResolver, syntax};
use std::collections::{BTreeMap, BTreeSet};

/// The actual generated .sym definitions, shared by decompilation and checking.
/// Numeric identities are never inferred from an unrelated equal-valued enum.
#[derive(Default)]
pub struct Definitions {
    symbols: BTreeMap<String, i32>,
    names: BTreeMap<(String, i32), String>,
    domains: BTreeSet<String>,
    field_flags: BTreeMap<(u16, i32), String>,
    field_flag_bases: BTreeMap<(u16, i32), String>,
    field_stages: BTreeMap<(u16, i32), String>,
    field_models: BTreeMap<(u16, i32), String>,
}

impl Definitions {
    pub fn from_sources(sources: &impl SourceResolver) -> Result<Self, Diagnostic> {
        let mut result = Self::default();
        for module in [
            "story",
            "characters",
            "monsters",
            "items",
            "fields",
            "music",
            "battle",
            "locations",
            "world",
            "effects",
            "treasure",
            "figurines",
            "scenario",
            "techniques",
            "recipes",
            "resources",
            "skits",
        ] {
            let path = format!("std::{module}");
            let Some(source) = sources.source(&path) else {
                continue;
            };
            let parsed = syntax::parse(&path, source)?;
            if parsed.kind != ScriptKind::Library {
                return Err(super::error(format!("{path} must declare script library")));
            }
            for enumeration in parsed.enums.into_iter().filter(|e| e.public) {
                let domain = format!("{module}::{}", enumeration.name);
                if !result.domains.insert(domain.clone()) {
                    return Err(enumeration.at.error("duplicate constant enum"));
                }
                let mut next = 0i64;
                for (name, payload, discriminant) in enumeration.variants {
                    if !payload.is_empty() {
                        continue;
                    }
                    let value = match discriminant {
                        Some(value) => number(&value)
                            .ok_or_else(|| enumeration.at.error("invalid enum discriminant"))?,
                        None => next,
                    };
                    let value = i32::try_from(value)
                        .map_err(|_| enumeration.at.error("scenario constants must fit i32"))?;
                    let symbol = format!("{domain}::{name}");
                    // An explicit scene enum is an opt-in declaration. Equal
                    // values in arbitrary enums never establish a scoped meaning.
                    if module == "story"
                        && let Some(field) = field_enum(&enumeration.name, "Flag")
                    {
                        result.field_flags.insert((field, value), domain.clone());
                    }
                    if module == "story"
                        && let Some(field) = field_enum(&enumeration.name, "FlagBase")
                    {
                        result
                            .field_flag_bases
                            .insert((field, value), domain.clone());
                    }
                    if module == "story"
                        && let Some(field) = field_enum(&enumeration.name, "StoryStage")
                    {
                        result.field_stages.insert((field, value), domain.clone());
                    }
                    if module == "resources"
                        && let Some(field) = field_enum(&enumeration.name, "ModelId")
                    {
                        result.field_models.insert((field, value), domain.clone());
                    }
                    if result.symbols.insert(symbol.clone(), value).is_some() {
                        return Err(enumeration.at.error("duplicate enum variant"));
                    }
                    if result
                        .names
                        .insert((domain.clone(), value), symbol)
                        .is_some()
                    {
                        return Err(enumeration.at.error("duplicate enum discriminant"));
                    }
                    next = i64::from(value) + 1;
                }
            }
        }
        Ok(result)
    }

    pub(super) fn require(&self, domain: &str) -> Result<(), Diagnostic> {
        if self.domains.contains(domain) {
            Ok(())
        } else {
            Err(super::error(format!(
                "missing std::{domain} definitions required for readable decompilation"
            )))
        }
    }

    pub(super) fn value(&self, symbol: &str) -> Option<i32> {
        self.symbols
            .get(symbol.strip_prefix("std::").unwrap_or(symbol))
            .copied()
    }

    pub(super) fn name(&self, domain: &str, value: i32) -> Option<&str> {
        self.names.get(&(domain.into(), value)).map(String::as_str)
    }

    pub(super) fn scoped_flag_domain(&self, context: super::Context, value: i32) -> Option<&str> {
        match context {
            super::Context::FieldScene(field) => {
                self.field_flags.get(&(field, value)).map(String::as_str)
            }
            super::Context::World => self
                .name("story::WorldFlag", value)
                .map(|_| "story::WorldFlag"),
            _ => None,
        }
    }

    pub(super) fn scoped_flag_base_domain(
        &self,
        context: super::Context,
        value: i32,
    ) -> Option<&str> {
        if let super::Context::FieldScene(field) = context {
            self.field_flag_bases
                .get(&(field, value))
                .map(String::as_str)
        } else {
            None
        }
    }

    pub(super) fn scoped_model_domain(&self, context: super::Context, value: i32) -> Option<&str> {
        if let super::Context::FieldScene(field) = context {
            self.field_models.get(&(field, value)).map(String::as_str)
        } else {
            None
        }
    }

    pub(super) fn scoped_value_domain<'a>(
        &'a self,
        domain: Option<&'a str>,
        context: super::Context,
        value: Option<i32>,
    ) -> Option<&'a str> {
        if domain == Some("story::StoryStage")
            && let super::Context::FieldScene(field) = context
            && let Some(value) = value
            && let Some(scoped) = self.field_stages.get(&(field, value))
        {
            return Some(scoped);
        }
        domain
    }

    pub(super) fn scoped_comparison_domain<'a>(
        &'a self,
        domain: Option<&'a str>,
        context: super::Context,
        value: Option<i32>,
        allow_bound_before: bool,
    ) -> Option<&'a str> {
        let exact = self.scoped_value_domain(domain, context, value)?;
        if allow_bound_before
            && let Some(value) = value
            && self.name(exact, value).is_none()
            && let Some(anchor) = value.checked_add(1)
            && let Some(scoped) = self.scoped_value_domain(domain, context, Some(anchor))
            && self.bound_before(scoped, value).is_some()
        {
            return Some(scoped);
        }
        Some(exact)
    }

    pub(super) fn bound_before(&self, domain: &str, value: i32) -> Option<String> {
        if !domain.starts_with("story::") || !domain.ends_with("Stage") {
            return None;
        }
        let anchor = self.name(domain, value.checked_add(1)?)?;
        Some(format!("bound_before({anchor})"))
    }

    pub(super) fn format(&self, domain: &str, value: i32) -> Option<String> {
        if let Some(name) = self.name(domain, value) {
            return Some(name.into());
        }
        if domain == "resources::VoiceArchiveReference" {
            let base = value & !0xffff;
            return self
                .name(domain, base)
                .map(|name| format!("voice_archive({name}, {})", value as u32 & 0xffff));
        }
        if !matches!(domain, "scenario::DialogueFlag" | "scenario::AnimationFlag") || value < 0 {
            return None;
        }
        let mut remaining = value as u32;
        let mut parts = Vec::new();
        for bit in 0..31 {
            let mask = 1u32 << bit;
            if remaining & mask != 0
                && let Some(name) = self.name(domain, mask as i32)
            {
                parts.push(name.to_owned());
                remaining &= !mask;
            }
        }
        if parts.is_empty() {
            return None;
        }
        if remaining != 0 {
            parts.push(format!("0x{remaining:X}"));
        }
        Some(format!("flags({})", parts.join(", ")))
    }
}

fn field_enum(name: &str, suffix: &str) -> Option<u16> {
    let digits = name.strip_prefix("Field")?.strip_suffix(suffix)?;
    (digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

pub(super) fn number(source: &str) -> Option<i64> {
    let source = source.replace('_', "");
    let (negative, source) = source
        .strip_prefix('-')
        .map_or((false, source.as_str()), |s| (true, s));
    let n = if let Some(hex) = source
        .strip_prefix("0x")
        .or_else(|| source.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()?
    } else {
        source.parse().ok()?
    };
    if negative { n.checked_neg() } else { Some(n) }
}
