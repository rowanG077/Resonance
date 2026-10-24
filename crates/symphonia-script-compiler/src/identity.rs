use crate::{
    Diagnostic,
    syntax::{self, BindingKind, Expression, Kind, TypeRef},
};
use std::collections::BTreeMap;

/// A conservative source identity independent of comments, indentation and
/// literal spelling. This validates syntax, not the module's imported types.
/// Asset declarations bind by symbol/type; their path spelling is not gameplay.
/// Explicit Message text is presentation, while ordinary strings stay significant
/// because hosts can use them as keys or behavioral inputs. Referenced asset
/// semantics must be fingerprinted separately by their content owner.
pub fn canonical_source(file: &str, source: &str) -> Result<Vec<u8>, Diagnostic> {
    let module = syntax::parse(file, source)?;
    let mut presentation = BTreeMap::new();
    for binding in &module.bindings {
        if !matches!(binding.value.kind, Expression::Text(_)) {
            continue;
        }
        let tag = if binding.kind == BindingKind::Asset {
            Some(6)
        } else if binding.kind == BindingKind::Constant
            && binding.ty == Some(TypeRef::Named("Message".into()))
        {
            Some(7)
        } else {
            None
        };
        if let Some(tag) = tag {
            presentation.insert((binding.value.at.line, binding.value.at.column), tag);
        }
    }
    for message in &module.messages {
        presentation.insert((message.text_at.line, message.text_at.column), 7);
    }
    let mut bytes = Vec::new();
    for token in syntax::lex(file, source)? {
        let (tag, value) = match token.kind {
            Kind::Comment(_) | Kind::End => continue,
            Kind::Word(text) => (0, text.into_bytes()),
            Kind::Symbol(text) => (1, text.into_bytes()),
            Kind::Text(text) => presentation
                .get(&(token.at.line, token.at.column))
                .map_or_else(|| (2, text.into_bytes()), |&tag| (tag, Vec::new())),
            Kind::Number(text) => {
                let text = text.replace('_', "");
                if let Some(ticks) = text.strip_suffix("ticks") {
                    let value: u64 = ticks
                        .parse()
                        .map_err(|_| token.at.error("invalid tick literal"))?;
                    (3, value.to_be_bytes().to_vec())
                } else if text.contains('.') {
                    let value: f32 = text
                        .parse()
                        .map_err(|_| token.at.error("invalid float literal"))?;
                    if !value.is_finite() {
                        return Err(token.at.error("non-finite float literal"));
                    }
                    (4, value.to_bits().to_be_bytes().to_vec())
                } else {
                    // The sign is a separate token. Retaining a u64 here also
                    // handles the magnitude of a valid negative i32 minimum.
                    let value = if let Some(hex) = text.strip_prefix("0x") {
                        u64::from_str_radix(hex, 16)
                    } else {
                        text.parse()
                    }
                    .map_err(|_| token.at.error("invalid integer literal"))?;
                    (5, value.to_be_bytes().to_vec())
                }
            }
        };
        bytes.push(tag);
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend(value);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_comments_and_literal_spelling_preserve_source_identity() {
        let first = "script library; pub const Count: i32 = 1_234; pub const Ratio: f32 = 1.5;";
        let second = "/* explanation */ script library;\n pub const Count:i32=0x4d2; // note\n pub const Ratio:f32=1.500;";
        assert_eq!(
            canonical_source("first", first).unwrap(),
            canonical_source("second", second).unwrap()
        );
        assert_ne!(
            canonical_source("first", first).unwrap(),
            canonical_source("first", &first.replace("1_234", "1_235")).unwrap()
        );
        assert_ne!(
            canonical_source("first", "script library; pub const T:string=\"a b\";").unwrap(),
            canonical_source("first", "script library; pub const T:string=\"ab\";").unwrap()
        );
        assert!(canonical_source("broken", "script library; pub const X = ;").is_err());
    }

    #[test]
    fn typed_asset_paths_and_display_text_keep_identity_but_bindings_and_logic_do_not() {
        let source = r#"script library;
            asset spark: game::Projectile = "effects/spark.json";
            message greeting = "Welcome.";
            pub const caption: Message = "A caption";
            message reward(amount: i32) = "Found {amount} Gald.";
            const key: string = "effects/spark.json";
            fn condition(value: string) -> bool { return value == "Welcome."; }
        "#;
        let identity = canonical_source("base", source).unwrap();
        let edited = source
            .replacen("effects/spark.json", "effects/renamed-spark.json", 1)
            .replacen("Welcome.", "Bienvenue.", 1)
            .replace("A caption", "A translated caption")
            .replace("Found {amount} Gald.", "{amount} Gald obtained.");
        assert_eq!(identity, canonical_source("edited", &edited).unwrap());
        for changed in [
            source.replace("spark:", "flash:"),
            source.replace("game::Projectile", "game::Image"),
            source.replace("amount: i32", "amount: text::Item"),
            source.replace("value == \"Welcome.\"", "value == \"Goodbye.\""),
            source.replace("const key: string", "const other: string"),
            source.replace(
                "string = \"effects/spark.json\"",
                "string = \"another-key\"",
            ),
        ] {
            assert_ne!(identity, canonical_source("changed", &changed).unwrap());
        }
        let state = "script library; state label: Message = \"Initial\";";
        assert_ne!(
            canonical_source("state", state).unwrap(),
            canonical_source("state", &state.replace("Initial", "Changed")).unwrap(),
            "state initializers remain conservative gameplay inputs"
        );
    }
}
