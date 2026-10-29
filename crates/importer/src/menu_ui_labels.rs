//! Player-menu labels read directly from the GameCube executable.
use crate::{dol, text};
use anyhow::{Result, bail, ensure};
use std::collections::BTreeMap;

fn table(
    exe: &[u8],
    labels: &mut BTreeMap<String, String>,
    address: u32,
    entries: &[(&str, u32)],
) -> Result<()> {
    for &(name, index) in entries {
        labels.insert(name.into(), text::pointer(exe, address + index * 4)?);
    }
    Ok(())
}

pub(crate) fn menu_labels(exe: &[u8]) -> Result<BTreeMap<String, String>> {
    let mut labels = BTreeMap::new();
    labels.insert("customize".into(), text::pointer(exe, 0x8019aa10)?);
    for (name, address) in [
        ("go_in", 0x8035af14),
        ("talk", 0x8035af1c),
        ("shop", 0x8035af24),
        ("examine", 0x8035af2c),
        ("open", 0x8035af34),
        ("push", 0x8035af3c),
        ("pull", 0x8035af44),
        ("operate", 0x8035af4c),
        ("switch", 0x8035af54),
        ("climb", 0x8035af5c),
        ("descend", 0x8035af64),
        ("jump", 0x8035af6c),
        ("hit", 0x8035af74),
        ("read", 0x8035af78),
        ("rest", 0x8035af80),
        ("eat", 0x8035af88),
        ("go_out", 0x8035af8c),
        ("move", 0x8035af94),
        ("grab", 0x8035af9c),
        ("question", 0x8035afa4),
        ("exclamation", 0x8035afa8),
        ("warp", 0x8035afb4),
    ] {
        labels.insert(name.into(), text::string(exe, address)?);
    }

    // Each route holds a text pointer followed by selection and destination IDs.
    for row in dol::slice(exe, 0x801aaad0, 10 * 8)?.chunks_exact(8) {
        let name = match dol::u16(row, 6)? {
            1 => "tech",
            2 => "strategy",
            3 => "equip",
            4 => "cooking",
            5 => "system",
            6 => "items",
            7 => "ex_skill",
            8 => "status",
            9 => "synopsis",
            11 => "unison",
            id => bail!("unknown menu destination {id}"),
        };
        labels.insert(name.into(), text::string(exe, dol::u32(row, 0)?)?);
    }
    table(
        exe,
        &mut labels,
        0x801aaa70,
        &[
            ("gald", 0),
            ("time", 1),
            ("encounter", 2),
            ("combo", 3),
            ("next", 4),
        ],
    )?;

    table(
        exe,
        &mut labels,
        0x8019bdc4,
        &[
            ("yes", 4),
            ("no", 5),
            ("empty", 20),
            ("play_time", 22),
            ("encounters", 23),
            ("max_combo", 24),
            ("load", 25),
            ("save", 26),
        ],
    )?;
    // Native confirmation prompts are in save/load/overwrite order. Each has a
    // pair of strings for memory card slots A and B.
    table(
        exe,
        &mut labels,
        0x8035cc78,
        &[
            ("confirm_save_a", 2),
            ("confirm_save_b", 3),
            ("confirm_load_a", 4),
            ("confirm_load_b", 5),
            ("confirm_overwrite_a", 6),
            ("confirm_overwrite_b", 7),
        ],
    )?;

    table(
        exe,
        &mut labels,
        0x801aadb0,
        &[
            ("shop_buy", 0),
            ("shop_sell", 1),
            ("shop_equip", 2),
            ("shop_exit", 3),
            ("shop_status", 4),
            ("shop_empty", 5),
            ("shop_confirm", 6),
            ("shop_yes", 7),
            ("shop_no", 8),
            ("shop_total", 9),
            ("shop_gald", 10),
            ("shop_select", 11),
            ("shop_add", 12),
            ("shop_reduce", 13),
            ("shop_ok", 14),
            ("shop_info", 16),
            ("shop_slash", 17),
            ("shop_thrust", 18),
            ("shop_defense", 19),
            ("shop_accuracy", 20),
            ("shop_evasion", 21),
            ("shop_intelligence", 22),
            ("shop_luck", 23),
            ("shop_attack", 24),
            ("shop_cannot_equip", 25),
        ],
    )?;
    ensure!(labels.len() == 77, "incomplete player-menu labels");
    Ok(labels)
}
