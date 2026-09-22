//! Technique, unison and party labels, menu routes and controller glyph bindings.
use super::text::{TextPool, TextRef};
use crate::{
    dol,
    read::{Field, u16 as half, u32 as word},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

const TECHNIQUE: u32 = 0x801aaf34;
const PARTY: u32 = 0x801aaa70;
const ROUTES: u32 = 0x801aaad0;
const UNISON_ASSIGNMENT: u32 = 0x801aafb8;
const TECHNIQUE_ASSIGNMENT: u32 = 0x801aafc4;
const UNISON_MENU: u32 = 0x801aafdc;
const UNISON_PAIR: u32 = 0x8035d5bc;
const TECHNIQUE_PAIR: u32 = 0x8035d5c4;
const PLAYERS: u32 = 0x8035d5cc;
const UNISON_MENU_PAIR: u32 = 0x8035d5d4;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub(crate) technique: Technique,
    pub(crate) party: Party,
    pub(crate) routes: [Route; 10],
    pub(crate) controls: Controls,
    pub(crate) unison_formats: UnisonFormats,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
}

super::text::record! {
    pub(crate) struct Attributes(r: TextRef) {
        pub(crate) strength: TextRef => r[0],
        pub(crate) slash: TextRef => r[1],
        pub(crate) thrust: TextRef => r[2],
        pub(crate) defense: TextRef => r[3],
        pub(crate) luck: TextRef => r[4],
        pub(crate) accuracy: TextRef => r[5],
        pub(crate) evasion: TextRef => r[6],
        pub(crate) intelligence: TextRef => r[7],
        pub(crate) attack: TextRef => r[8],
    }
}

super::text::record! {
    pub(crate) struct Technique(r: TextRef) {
        pub(crate) title: TextRef => r[0],
        pub(crate) tp_cost: TextRef => r[1],
        pub(crate) usage: TextRef => r[2],
        pub(crate) remove: TextRef => r[3],
        pub(crate) auto: TextRef => r[4],
        pub(crate) execute: TextRef => r[5],
        pub(crate) forget: TextRef => r[6],
        pub(crate) unison_settings: TextRef => r[7],
        pub(crate) control_type: TextRef => r[8],
        pub(crate) manual: TextRef => r[9],
        pub(crate) semi_auto: TextRef => r[10],
        pub(crate) auto_mode: TextRef => r[11],
        pub(crate) select: TextRef => r[12],
        pub(crate) shortcut: TextRef => r[13],
        pub(crate) unison_setting: TextRef => r[14],
        pub(crate) attributes: Attributes => Attributes::from_refs(&r[15..24]),
        pub(crate) target: TextRef => r[24],
        pub(crate) target_all: TextRef => r[25],
        pub(crate) cannot_forget: TextRef => r[26],
        pub(crate) related: TextRef => r[27],
        pub(crate) forget_warning: TextRef => r[28],
        pub(crate) forget_confirm: TextRef => r[29],
        /// Yes, no, in the authored choice order.
        pub(crate) confirmation: [TextRef; 2] => [r[30], r[31]],
        pub(crate) unison_title: TextRef => r[32],
    }
}

super::text::record! {
    pub(crate) struct Party(r: TextRef) {
        pub(crate) gald: TextRef => r[0],
        pub(crate) time: TextRef => r[1],
        pub(crate) encounters: TextRef => r[2],
        pub(crate) combo: TextRef => r[3],
        pub(crate) next: TextRef => r[4],
        pub(crate) slash: TextRef => r[5],
        pub(crate) thrust: TextRef => r[6],
        pub(crate) attack: TextRef => r[7],
        pub(crate) defense: TextRef => r[8],
        pub(crate) luck: TextRef => r[9],
        pub(crate) accuracy: TextRef => r[10],
        pub(crate) evasion: TextRef => r[11],
        pub(crate) exchange_target: TextRef => r[12],
        pub(crate) display_change: TextRef => r[13],
        pub(crate) exchange: TextRef => r[14],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Selection {
    Direct = 0,
    Character = 1,
    System = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Destination {
    Tech = 1,
    Strategy = 2,
    Equip = 3,
    Cooking = 4,
    System = 5,
    Items = 6,
    ExSkill = 7,
    Status = 8,
    Synopsis = 9,
    Unison = 11,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Route {
    pub(crate) label: TextRef,
    pub(crate) selection: Selection,
    /// The same native ID selects availability bit (id - 1).
    pub(crate) destination: Destination,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Glyphs<T> {
    pub(crate) normal: T,
    pub(crate) highlight: T,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Assignment<T> {
    /// Slots other than the fourth use one glyph each.
    pub(crate) single: Glyphs<T>,
    /// The fourth assignment slot uses two glyphs side by side.
    pub(crate) paired: Glyphs<[u16; 2]>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Controls {
    pub(crate) unison_assignment: Assignment<[u16; 3]>,
    /// Six stored slots; the fourth single glyph is retained although a pair is drawn.
    pub(crate) technique_assignment: Assignment<[u16; 6]>,
    pub(crate) unison_menu: Assignment<[u16; 3]>,
    /// Party position selects the glyph; the active position is highlighted.
    pub(crate) players: Glyphs<[u16; 4]>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct UnisonFormats {
    pub(crate) number: TextRef,
    pub(crate) player: TextRef,
}

fn glyphs<const N: usize>(executable: &[u8], address: u32) -> Result<Glyphs<[u16; N]>> {
    let source = dol::slice(executable, address, N * 4)?;
    Ok(Glyphs {
        normal: Field::read(source, 0)?,
        highlight: Field::read(source, N * 2)?,
    })
}

fn assignment<const N: usize>(
    executable: &[u8],
    single: u32,
    paired: u32,
) -> Result<Assignment<[u16; N]>> {
    Ok(Assignment {
        single: glyphs(executable, single)?,
        paired: glyphs(executable, paired)?,
    })
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    // The remainder of the 0xa8-byte symbol consists of controller halfwords.
    let t = texts.required_table(executable, TECHNIQUE, 33)?;
    let p = texts.required_table(executable, PARTY, 15)?;
    let routes: Vec<_> = dol::slice(executable, ROUTES, 10 * 8)?
        .chunks_exact(8)
        .map(|row| {
            let selection = match half(row, 4)? {
                0 => Selection::Direct,
                1 => Selection::Character,
                2 => Selection::System,
                value => bail!("unknown menu selection mode {value}"),
            };
            let destination = match half(row, 6)? {
                1 => Destination::Tech,
                2 => Destination::Strategy,
                3 => Destination::Equip,
                4 => Destination::Cooking,
                5 => Destination::System,
                6 => Destination::Items,
                7 => Destination::ExSkill,
                8 => Destination::Status,
                9 => Destination::Synopsis,
                11 => Destination::Unison,
                value => bail!("unknown menu destination {value}"),
            };
            Ok(Route {
                label: texts.required(executable, word(row, 0)?)?,
                selection,
                destination,
            })
        })
        .collect::<Result<_>>()?;
    let source = dol::slice(executable, PLAYERS, 8)?;
    let highlight: [u16; 4] = Field::read(source, 0)?;
    // The renderer subtracts one exactly when this party position is not selected.
    let players = Glyphs {
        normal: highlight.map(|glyph| glyph.wrapping_sub(1)),
        highlight,
    };
    let unison_formats = UnisonFormats {
        number: texts.required(executable, 0x8035d5dc)?,
        player: texts.required(executable, 0x8035d5e0)?,
    };
    Ok(Catalogue {
        technique: Technique::from_refs(&t),
        party: Party::from_refs(&p),
        routes: routes.try_into().unwrap(),
        controls: Controls {
            unison_assignment: assignment(executable, UNISON_ASSIGNMENT, UNISON_PAIR)?,
            technique_assignment: assignment(executable, TECHNIQUE_ASSIGNMENT, TECHNIQUE_PAIR)?,
            unison_menu: assignment(executable, UNISON_MENU, UNISON_MENU_PAIR)?,
            players,
        },
        unison_formats,
        texts: texts.values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_technique_ui_preserves_text_routes_and_controller_bindings() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = read(&executable)?;
            let encoded = serde_json::to_vec(&catalogue)?;
            let restored: Catalogue = serde_json::from_slice(&encoded)?;
            assert_eq!(restored, catalogue);
            if let Some(expected) = &first {
                assert_eq!(&encoded, expected);
            } else {
                first = Some(encoded);
            }
            let c = &restored.controls;
            for assignment in [&c.unison_assignment, &c.unison_menu] {
                assert_eq!(assignment.single.normal, [0, 1, 2]);
                assert_eq!(assignment.single.highlight, [0, 33, 34]);
            }
            assert_eq!(c.technique_assignment.single.normal, [0, 1, 2, 32, 25, 26]);
            assert_eq!(
                c.technique_assignment.single.highlight,
                [0, 33, 34, 32, 37, 38]
            );
            for pair in [
                &c.unison_assignment.paired,
                &c.technique_assignment.paired,
                &c.unison_menu.paired,
            ] {
                assert_eq!(pair.normal, [4, 3]);
                assert_eq!(pair.highlight, [35, 36]);
            }
            assert_eq!(c.players.normal, [5, 7, 9, 11]);
            assert_eq!(c.players.highlight, [6, 8, 10, 12]);
            // lhz; two subf/or/shift implement slot inequality; subf; u16 truncation.
            for (address, expected) in [
                (0x800c9120, 0xa0140000),
                (0x800c9124, 0x7ca4f850),
                (0x800c9128, 0x7c9f2050),
                (0x800c912c, 0x7ca52378),
                (0x800c9134, 0x54a50ffe),
                (0x800c913c, 0x7c050050),
                (0x800c9144, 0x5405043e),
                (0x800c9328, 0x3a940002),
            ] {
                assert_eq!(word(dol::slice(&executable, address, 4)?, 0)?, expected);
            }
            assert_eq!(restored.text(restored.technique.tp_cost), "TP : \x0c\x09%d");
            assert_eq!(restored.text(restored.unison_formats.number), "%d");
            assert_eq!(restored.text(restored.unison_formats.player), "%dP");
            assert_eq!(restored.routes[0].selection, Selection::Character);
            assert_eq!(restored.routes[9].selection, Selection::System);
            assert_eq!(
                restored
                    .routes
                    .each_ref()
                    .map(|route| route.destination as u16),
                [1, 11, 2, 8, 9, 6, 7, 3, 4, 5]
            );

            // Text control operands can be zero; unselected glyphs wrap as u16.
            let title = word(dol::slice(&executable, TECHNIQUE, 4)?, 0)?;
            let tp_cost = word(dol::slice(&executable, TECHNIQUE + 4, 4)?, 0)?;
            for (address, replacement) in [
                (TECHNIQUE + 2 * 4, title.to_be_bytes().to_vec()),
                (tp_cost, b"TP : \x0c\0%d\0".to_vec()),
                (PLAYERS, 0u16.to_be_bytes().to_vec()),
            ] {
                let slice = dol::slice(&executable, address, replacement.len())?;
                let offset = slice.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
            }
            let changed = read(&executable)?;
            assert_eq!(changed.technique.title, changed.technique.usage);
            assert_eq!(changed.text(changed.technique.tp_cost), "TP : \x0c\0%d");
            assert_eq!(changed.controls.players.normal[0], u16::MAX);
            for address in [ROUTES + 4, ROUTES + 6] {
                let at = dol::slice(&executable, address, 2)?.as_ptr() as usize
                    - executable.as_ptr() as usize;
                let original = [executable[at], executable[at + 1]];
                executable[at..at + 2].copy_from_slice(&u16::MAX.to_be_bytes());
                assert!(read(&executable).is_err());
                executable[at..at + 2].copy_from_slice(&original);
            }
        }
        Ok(())
    }
}
