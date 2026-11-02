//! Native menu captions, with numbered saves normalized to the shared A/B banks.
use crate::{pe, source::Source, text};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(crate) fn character_names(
    image: &pe::Image<'_>,
    bank: &text::Bank<'_>,
    slot: u32,
) -> Result<Vec<String>> {
    let names = pe::word(image.slice(0x8cf10c + slot * 4, 4)?, 0)?;
    (0..9)
        .map(|i| {
            bank.menu(
                pe::word(image.slice(names + i * 4, 4)?, 0)?,
                encoding_rs::WINDOWS_1252,
            )
        })
        .collect()
}

pub(crate) fn read(
    source: &Source,
    executable: &pe::Image<'_>,
    bank: &text::Bank<'_>,
    slot: u32,
) -> Result<BTreeMap<String, String>> {
    let word = |address| pe::word(executable.slice(address, 4)?, 0);
    let mut labels = BTreeMap::new();
    labels.extend(shop(executable, bank, slot)?);
    for (directory, keys) in [
        (
            0x8b9328,
            &[
                "tech",
                "unison",
                "strategy",
                "status",
                "synopsis",
                "items",
                "ex_skill",
                "equip",
                "cooking",
                "system",
                "save",
                "load",
                "customize",
            ][..],
        ),
        (
            0x8b9310,
            &["gald", "time", "encounter", "combo", "next"][..],
        ),
    ] {
        let table = word(directory + slot * 4)?;
        for (index, key) in keys.iter().enumerate() {
            labels.insert(
                (*key).into(),
                bank.menu(word(table + index as u32 * 4)?, encoding_rs::WINDOWS_1252)?,
            );
        }
    }
    labels.extend(save(source)?);
    // Localized field-action captions are inline arrays of 26 text IDs.
    for (index, key) in [
        (0, "go_in"),
        (1, "talk"),
        (3, "shop"),
        (4, "examine"),
        (5, "open"),
        (6, "push"),
        (7, "pull"),
        (8, "operate"),
        (9, "switch"),
        (10, "climb"),
        (11, "descend"),
        (12, "jump"),
        (13, "hit"),
        (14, "read"),
        (15, "rest"),
        (16, "eat"),
        (17, "go_out"),
        (18, "move"),
        (19, "grab"),
        (20, "question"),
        (21, "exclamation"),
        (23, "warp"),
    ] {
        labels.insert(
            key.into(),
            bank.menu(
                word(0x8d7e8c + (slot * 26 + index) * 4)?,
                encoding_rs::WINDOWS_1252,
            )?,
        );
    }
    Ok(labels)
}

fn shop(
    image: &pe::Image<'_>,
    bank: &text::Bank<'_>,
    slot: u32,
) -> Result<BTreeMap<String, String>> {
    let word = |address| pe::word(image.slice(address, 4)?, 0);
    let mut labels = BTreeMap::new();
    // Native shop drawing at 0x51c79a / 0x51d741 / 0x51db62 selects these
    // language directories. Comparison captions use the shop's short labels.
    for (directory, keys) in [
        (
            0x8b88e4,
            &[(0, "buy"), (1, "sell"), (2, "equip"), (3, "exit")][..],
        ),
        (
            0x8b88fc,
            &[
                (0, "slash"),
                (1, "thrust"),
                (2, "defense"),
                (3, "accuracy"),
                (4, "evasion"),
                (5, "intelligence"),
                (6, "luck"),
            ][..],
        ),
        (
            0x8b8914,
            &[
                (0, "status"),
                (1, "empty"),
                (2, "confirm"),
                (3, "yes"),
                (4, "no"),
                (5, "total"),
                (6, "gald"),
                (7, "select"),
                (8, "add"),
                (9, "reduce"),
                (10, "ok"),
                (11, "info"),
                (15, "attack"),
                (17, "cannot_equip"),
            ][..],
        ),
    ] {
        let table = word(directory + slot * 4)?;
        for &(index, key) in keys {
            labels.insert(
                format!("shop_{key}"),
                bank.menu(word(table + index * 4)?, encoding_rs::WINDOWS_1252)?,
            );
        }
    }
    Ok(labels)
}

fn save(source: &Source) -> Result<BTreeMap<String, String>> {
    let source = localization(source)?;
    let caption = |key: &str| -> Result<&str> {
        let text = source
            .get(key)
            .with_context(|| format!("missing Steam save label {key}"))?;
        ensure!(
            !text.is_empty() && text.chars().all(|ch| !ch.is_control()),
            "invalid Steam save label {key}"
        );
        Ok(text)
    };
    let mut labels = BTreeMap::new();
    for (key, native) in [
        ("save", "mn_data_save"),
        ("load", "mn_data_load"),
        ("empty", "mn_data_nodata"),
        ("gald", "mn_data_gald"),
        ("play_time", "mn_data_playtime"),
        ("encounters", "mn_data_encounter"),
        ("max_combo", "mn_data_maxcombo"),
        ("yes", "mn_custom_text_yes"),
        ("no", "mn_custom_text_no"),
    ] {
        labels.insert(key.into(), caption(native)?.into());
    }
    // Native drawing at 0x5065c7 formats the selected numbered save into these
    // keys. Resonance's existing contract instead selects one caption per bank.
    for action in ["save", "load", "overwrite"] {
        let text = caption(&format!("mn_data_{action}confirm"))?;
        ensure!(
            text.matches('%').count() == 1 && text.contains("%d"),
            "unexpected Steam save prompt format"
        );
        for (key, bank) in [("a", "A"), ("b", "B")] {
            labels.insert(format!("confirm_{action}_{key}"), text.replace("%d", bank));
        }
    }
    Ok(labels)
}

fn localization(source: &Source) -> Result<BTreeMap<String, String>> {
    let resource = source.require("LOC.TXT")?;
    let (text, invalid) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(&resource);
    ensure!(!invalid, "invalid Steam menu localization encoding");
    let rows = text.lines().map(row).collect::<Result<Vec<_>>>()?;
    ensure!(rows.len() == 7, "unexpected Steam localization languages");
    let keys = &rows[0];
    ensure!(
        !keys.is_empty() && rows.iter().all(|row| row.len() == keys.len()),
        "inconsistent Steam localization columns"
    );
    // Header, unused Japanese row, then the five selectable text languages.
    let index = source.language.slot() as usize + 1;
    let mut labels = BTreeMap::new();
    for (key, value) in keys.iter().zip(&rows[index]) {
        ensure!(
            !key.is_empty() && labels.insert(key.clone(), value.trim().into()).is_none(),
            "duplicate Steam localization key {key}"
        );
    }
    Ok(labels)
}

fn row(line: &str) -> Result<Vec<String>> {
    let mut chars = line.chars().peekable();
    let mut cells = Vec::new();
    loop {
        let mut cell = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            loop {
                match chars
                    .next()
                    .context("unterminated Steam localization quote")?
                {
                    '"' if chars.peek() == Some(&'"') => {
                        chars.next();
                        cell.push('"');
                    }
                    '"' => break,
                    ch => cell.push(ch),
                }
            }
        } else {
            while chars.peek().is_some_and(|&ch| ch != ',') {
                let ch = chars.next().unwrap();
                ensure!(ch != '"', "unexpected Steam localization quote");
                cell.push(ch);
            }
        }
        cells.push(cell);
        match chars.next() {
            None => return Ok(cells),
            Some(',') => (),
            Some(_) => anyhow::bail!("text after Steam localization quote"),
        }
    }
}
