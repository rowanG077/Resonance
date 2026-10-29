//! Source labels become short editing names; source tags stay importer-only.
use std::collections::BTreeSet;

pub(super) fn name(source: &str, used: &mut BTreeSet<String>) -> String {
    let source = source
        .split(['_'])
        .take_while(|part| {
            !part.starts_with("Pow") && !part.starts_with("Bb") && !part.starts_with("Dt")
        })
        .filter(|part| {
            !part.is_empty() && !matches!(*part, "AB" | "ROOT" | "ROOT01" | "FP" | "NR" | "01")
        })
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase();
    let source = source
        .replace("rkami", "kami-right")
        .replace("lkami", "kami-left");
    let mut pieces = Vec::new();
    for part in source.split('-') {
        let digits = part
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(part.len());
        let (word, number) = part.split_at(digits);
        let word = match word {
            "kami" | "hair" => "hair",
            "manto" | "mant" => "cape",
            "nuno" => "cloth",
            "kata" => "shoulder-cloth",
            "koshi" | "kosi" => "waist-cloth",
            "osage" => "braid",
            "momi" => "sideburns",
            "hige" => "beard",
            "mayuge" => "eyebrows",
            "mimi" => "ear",
            "sippo" => "tail",
            "hachimaki" => "headband",
            "fbelt" => "front-belt",
            "fusa" => "tassel",
            "non" => "unidentified",
            "l" => "left",
            "r" => "right",
            "b" => "back",
            "f" => "front",
            other => other,
        };
        if matches!(word, "hane" | "nemoto" | "kzl" | "kzr") {
            pieces.push("unidentified".into());
        }
        if !word.is_empty() {
            pieces.push(word.to_owned());
        }
        if !number.is_empty() {
            pieces.push(
                number
                    .parse::<u32>()
                    .map_or_else(|_| number.to_owned(), |n| n.to_string()),
            );
        }
    }
    let stem = if pieces.is_empty() {
        "unidentified".into()
    } else {
        pieces.join("-")
    };
    let mut label = stem.clone();
    let mut variant = 2;
    while !used.insert(label.clone()) {
        label = format!("{stem}-{variant}");
        variant += 1;
    }
    label
}
