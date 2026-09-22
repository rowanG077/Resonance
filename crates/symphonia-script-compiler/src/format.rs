use crate::{
    Diagnostic,
    syntax::{Kind, lex, parse},
};

/// Normalize indentation and spacing while preserving every comment and text
/// literal. The formatter first parses, so invalid input is never rewritten.
pub fn format(file: &str, source: &str) -> Result<String, Diagnostic> {
    parse(file, source)?;
    let mut output = String::new();
    let mut indent = 0usize;
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    let mut previous = String::new();
    let mut line_start = true;
    let tokens = lex(file, source)?;
    for (index, token) in tokens.iter().enumerate() {
        let text = match &token.kind {
            Kind::End => break,
            Kind::Word(v) | Kind::Number(v) | Kind::Symbol(v) | Kind::Comment(v) => v.clone(),
            Kind::Text(v) => quote(v),
        };
        if text == "}" {
            indent = indent.saturating_sub(1);
        }
        if line_start {
            output.push_str(&"    ".repeat(indent));
            line_start = false;
        }
        if matches!(token.kind, Kind::Comment(_)) {
            if !output.is_empty() && !output.ends_with([' ', '\n']) {
                output.push(' ');
            }
            output.push_str(&text);
            output.push('\n');
            line_start = true;
            previous.clear();
            continue;
        }
        let tight_before = matches!(text.as_str(), ";" | "," | ")" | "]" | "::" | "." | ":")
            || text == "(" && !matches!(previous.as_str(), "if" | "while" | "match")
            || text == "[" && !matches!(previous.as_str(), "=" | ":" | "return");
        let tight_after = matches!(previous.as_str(), "(" | "[" | "::" | "." | "!" | "");
        if !tight_before && !tight_after && !output.ends_with([' ', '\n']) {
            output.push(' ');
        }
        output.push_str(&text);
        match text.as_str() {
            "(" => parentheses += 1,
            ")" => parentheses -= 1,
            "[" => brackets += 1,
            "]" => brackets -= 1,
            "{" => {
                indent += 1;
                output.push('\n');
                line_start = true;
            }
            "}" => {
                let next = tokens.get(index + 1);
                if !next.is_some_and(|t| t.is(";") || t.is(",") || t.is(")") || t.is("else")) {
                    output.push('\n');
                    line_start = true;
                }
            }
            ";" if parentheses == 0 && brackets == 0 => {
                output.push('\n');
                line_start = true;
            }
            "," if parentheses == 0 && brackets == 0 => {
                output.push('\n');
                line_start = true;
            }
            _ => {}
        }
        previous = text;
    }
    while output.ends_with([' ', '\n']) {
        output.pop();
    }
    output.push('\n');
    Ok(output)
}

fn quote(value: &str) -> String {
    let mut text = String::from("\"");
    for character in value.chars() {
        match character {
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            '\\' => text.push_str("\\\\"),
            '"' => text.push_str("\\\""),
            character => text.push(character),
        }
    }
    text.push('"');
    text
}
