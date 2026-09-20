//! Technique, unison and party labels, menu routes and controller glyph bindings.
use super::text::{TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "technique-ui";
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Attributes {
    pub(crate) strength: TextRef,
    pub(crate) slash: TextRef,
    pub(crate) thrust: TextRef,
    pub(crate) defense: TextRef,
    pub(crate) luck: TextRef,
    pub(crate) accuracy: TextRef,
    pub(crate) evasion: TextRef,
    pub(crate) intelligence: TextRef,
    pub(crate) attack: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Technique {
    pub(crate) title: TextRef,
    pub(crate) tp_cost: TextRef,
    pub(crate) usage: TextRef,
    pub(crate) remove: TextRef,
    pub(crate) auto: TextRef,
    pub(crate) execute: TextRef,
    pub(crate) forget: TextRef,
    pub(crate) unison_settings: TextRef,
    pub(crate) control_type: TextRef,
    pub(crate) manual: TextRef,
    pub(crate) semi_auto: TextRef,
    pub(crate) auto_mode: TextRef,
    pub(crate) select: TextRef,
    pub(crate) shortcut: TextRef,
    pub(crate) unison_setting: TextRef,
    pub(crate) attributes: Attributes,
    pub(crate) target: TextRef,
    pub(crate) target_all: TextRef,
    pub(crate) cannot_forget: TextRef,
    pub(crate) related: TextRef,
    pub(crate) forget_warning: TextRef,
    pub(crate) forget_confirm: TextRef,
    /// Yes, no, in the authored choice order.
    pub(crate) confirmation: [TextRef; 2],
    pub(crate) unison_title: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Party {
    pub(crate) gald: TextRef,
    pub(crate) time: TextRef,
    pub(crate) encounters: TextRef,
    pub(crate) combo: TextRef,
    pub(crate) next: TextRef,
    pub(crate) slash: TextRef,
    pub(crate) thrust: TextRef,
    pub(crate) attack: TextRef,
    pub(crate) defense: TextRef,
    pub(crate) luck: TextRef,
    pub(crate) accuracy: TextRef,
    pub(crate) evasion: TextRef,
    pub(crate) exchange_target: TextRef,
    pub(crate) display_change: TextRef,
    pub(crate) exchange: TextRef,
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
    let row = |offset| {
        std::array::from_fn(|i| {
            u16::from_be_bytes(
                source[offset + i * 2..offset + i * 2 + 2]
                    .try_into()
                    .unwrap(),
            )
        })
    };
    Ok(Glyphs {
        normal: row(0),
        highlight: row(N * 2),
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

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
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
    let highlight: [u16; 4] =
        std::array::from_fn(|i| u16::from_be_bytes(source[i * 2..i * 2 + 2].try_into().unwrap()));
    // The renderer subtracts one exactly when this party position is not selected.
    let players = Glyphs {
        normal: highlight.map(|glyph| glyph.wrapping_sub(1)),
        highlight,
    };
    let unison_formats = UnisonFormats {
        number: texts.required(executable, 0x8035d5dc)?,
        player: texts.required(executable, 0x8035d5e0)?,
    };
    Ok((
        Catalogue {
            technique: Technique {
                title: t[0],
                tp_cost: t[1],
                usage: t[2],
                remove: t[3],
                auto: t[4],
                execute: t[5],
                forget: t[6],
                unison_settings: t[7],
                control_type: t[8],
                manual: t[9],
                semi_auto: t[10],
                auto_mode: t[11],
                select: t[12],
                shortcut: t[13],
                unison_setting: t[14],
                attributes: Attributes {
                    strength: t[15],
                    slash: t[16],
                    thrust: t[17],
                    defense: t[18],
                    luck: t[19],
                    accuracy: t[20],
                    evasion: t[21],
                    intelligence: t[22],
                    attack: t[23],
                },
                target: t[24],
                target_all: t[25],
                cannot_forget: t[26],
                related: t[27],
                forget_warning: t[28],
                forget_confirm: t[29],
                confirmation: [t[30], t[31]],
                unison_title: t[32],
            },
            party: Party {
                gald: p[0],
                time: p[1],
                encounters: p[2],
                combo: p[3],
                next: p[4],
                slash: p[5],
                thrust: p[6],
                attack: p[7],
                defense: p[8],
                luck: p[9],
                accuracy: p[10],
                evasion: p[11],
                exchange_target: p[12],
                display_change: p[13],
                exchange: p[14],
            },
            routes: routes.try_into().unwrap(),
            controls: Controls {
                unison_assignment: assignment(executable, UNISON_ASSIGNMENT, UNISON_PAIR)?,
                technique_assignment: assignment(executable, TECHNIQUE_ASSIGNMENT, TECHNIQUE_PAIR)?,
                unison_menu: assignment(executable, UNISON_MENU, UNISON_MENU_PAIR)?,
                players,
            },
            unison_formats,
            texts: texts.values,
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, sources) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "technique":{"address":TECHNIQUE,"count":33,"stride":4,"source_size":132},
            "party":{"address":PARTY,"count":15,"stride":4,"source_size":60},
            "routes":{"address":ROUTES,"count":10,"stride":8,"source_size":80},
            "controller_glyphs": [
                {"role":"unison_assignment","address":UNISON_ASSIGNMENT,"source_size":12},
                {"role":"technique_assignment","address":TECHNIQUE_ASSIGNMENT,"source_size":24},
                {"role":"unison_menu","address":UNISON_MENU,"source_size":12},
                {"role":"unison_assignment_pair","address":UNISON_PAIR,"source_size":8},
                {"role":"technique_assignment_pair","address":TECHNIQUE_PAIR,"source_size":8},
                {"role":"player_highlights","address":PLAYERS,"source_size":8},
                {"role":"unison_menu_pair","address":UNISON_MENU_PAIR,"source_size":8},
            ],
            "player_glyph_selection":{"address":0x800c9114u32,"source_size":52},
            "texts":sources,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn text_bytes(text: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if matches!(c, '\x0b' | '\x0c') {
                bytes.extend([c as u8, u8::try_from(chars.next().unwrap() as u32).unwrap()]);
            } else {
                let text = c.to_string();
                let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&text);
                assert!(!invalid, "unrepresentable source character {c:?}");
                bytes.extend_from_slice(&encoded);
            }
        }
        bytes.push(0);
        bytes
    }

    fn refs(c: &Catalogue) -> ([TextRef; 33], [TextRef; 15]) {
        let t = &c.technique;
        let a = &t.attributes;
        let p = &c.party;
        (
            [
                t.title,
                t.tp_cost,
                t.usage,
                t.remove,
                t.auto,
                t.execute,
                t.forget,
                t.unison_settings,
                t.control_type,
                t.manual,
                t.semi_auto,
                t.auto_mode,
                t.select,
                t.shortcut,
                t.unison_setting,
                a.strength,
                a.slash,
                a.thrust,
                a.defense,
                a.luck,
                a.accuracy,
                a.evasion,
                a.intelligence,
                a.attack,
                t.target,
                t.target_all,
                t.cannot_forget,
                t.related,
                t.forget_warning,
                t.forget_confirm,
                t.confirmation[0],
                t.confirmation[1],
                t.unison_title,
            ],
            [
                p.gald,
                p.time,
                p.encounters,
                p.combo,
                p.next,
                p.slash,
                p.thrust,
                p.attack,
                p.defense,
                p.luck,
                p.accuracy,
                p.evasion,
                p.exchange_target,
                p.display_change,
                p.exchange,
            ],
        )
    }

    fn glyph_bytes<const N: usize>(glyphs: &Glyphs<[u16; N]>) -> Vec<u8> {
        glyphs
            .normal
            .iter()
            .chain(&glyphs.highlight)
            .flat_map(|v| v.to_be_bytes())
            .collect()
    }

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_technique_ui_preserves_text_routes_and_controller_bindings() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let (catalogue, sources) = parse(&executable)?;
            let encoded = serde_json::to_vec(&catalogue)?;
            let restored: Catalogue = serde_json::from_slice(&encoded)?;
            assert_eq!(restored, catalogue);
            if let Some(expected) = &first {
                assert_eq!(&encoded, expected);
            } else {
                first = Some(encoded);
            }
            let pointers = |references: &[TextRef]| {
                references
                    .iter()
                    .flat_map(|id| sources[id.0].address.to_be_bytes())
                    .collect::<Vec<_>>()
            };
            let (technique, party) = refs(&restored);
            assert_eq!(
                pointers(&technique),
                dol::slice(&executable, TECHNIQUE, 132)?
            );
            assert_eq!(pointers(&party), dol::slice(&executable, PARTY, 60)?);
            let routes: Vec<_> = restored
                .routes
                .iter()
                .flat_map(|route| {
                    sources[route.label.0]
                        .address
                        .to_be_bytes()
                        .into_iter()
                        .chain((route.selection as u16).to_be_bytes())
                        .chain((route.destination as u16).to_be_bytes())
                })
                .collect();
            assert_eq!(routes, dol::slice(&executable, ROUTES, 80)?);
            for (source, text) in sources.iter().zip(&restored.texts) {
                let bytes = text_bytes(text);
                assert_eq!(bytes.len() as u32, source.source_size);
                assert_eq!(
                    bytes,
                    dol::slice(&executable, source.address, source.source_size as usize)?
                );
            }
            let c = &restored.controls;
            for (address, bytes) in [
                (UNISON_ASSIGNMENT, glyph_bytes(&c.unison_assignment.single)),
                (
                    TECHNIQUE_ASSIGNMENT,
                    glyph_bytes(&c.technique_assignment.single),
                ),
                (UNISON_MENU, glyph_bytes(&c.unison_menu.single)),
                (UNISON_PAIR, glyph_bytes(&c.unison_assignment.paired)),
                (TECHNIQUE_PAIR, glyph_bytes(&c.technique_assignment.paired)),
                (UNISON_MENU_PAIR, glyph_bytes(&c.unison_menu.paired)),
                (
                    PLAYERS,
                    c.players
                        .highlight
                        .iter()
                        .flat_map(|v| v.to_be_bytes())
                        .collect(),
                ),
            ] {
                assert_eq!(bytes, dol::slice(&executable, address, bytes.len())?);
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

            // Pool by source address, preserve zero-valued color operands and
            // apply the native u16 arithmetic even to the boundary glyph value.
            for (address, replacement) in [
                (
                    TECHNIQUE + 2 * 4,
                    sources[restored.technique.title.0]
                        .address
                        .to_be_bytes()
                        .to_vec(),
                ),
                (
                    sources[restored.technique.tp_cost.0].address,
                    b"TP : \x0c\0%d\0".to_vec(),
                ),
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
        }
        Ok(())
    }
}
