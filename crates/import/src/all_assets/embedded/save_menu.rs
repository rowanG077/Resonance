//! Save/load text, shared references and authored confirmation choices.
use super::text::{TextPool, TextRef};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

const COMMON: u32 = 0x8019bdc4;
const MESSAGES: u32 = 0x8035cc78;
const CHOICES: u32 = 0x8035cdc0;

super::ordered! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[repr(u8)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum CommonLabel {
        BuyCancel,
        Buy,
        End,
        ConfirmInheritance,
        Yes,
        No,
        InsertCard,
        NoCard,
        Deleting,
        ConfirmDelete,
        LoadComplete,
        FinishSave,
        Back,
        ContinueWithoutSaving,
        Retry,
        ContinueWithoutSavingAlternate,
        Format,
        SpaceRequirement,
        SlotFormat,
        Destroy,
        NoData,
        Gald,
        PlayTime,
        Encounters,
        MaxCombo,
        Load,
        Save,
        NoCardA,
        NoCardB,
        ContinueWithoutLoading,
        RetryLoading,
        DeleteFile,
        MissingFile,
        RenameExistingFile,
        DirectoryFull,
        NoFreeBlocks,
        AccessDenied,
        AccessPastEnd,
        FileNameTooLong,
        WrongEncoding,
        OtherErrors,
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    /// Indexed references preserve aliases by source pointer, not equal wording.
    texts: Vec<String>,
    common: Vec<CommonText>,
    card_messages: Vec<CardMessage>,
    format_card_choices: Choices,
    corrupt_file_choices: Choices,
    labels: BTreeMap<Label, TextRef>,
    default_error_reference: DefaultErrorReference,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DefaultErrorReference {
    byte_offset: i16,
    /// The default can address data beyond the common-message pointer table.
    target_word: u32,
}

impl Catalogue {
    pub(crate) fn common(&self, label: CommonLabel) -> &str {
        &self.texts[self.common[label as usize].text.0]
    }

    pub(crate) fn prompts(&self) -> impl Iterator<Item = (Prompt, [&str; 2])> {
        self.card_messages
            .iter()
            .filter_map(|message| match message {
                CardMessage::Slots { prompt, texts } => Some((
                    *prompt,
                    texts.map(|reference| self.texts[reference.0].as_str()),
                )),
                CardMessage::Accessing { .. } => None,
            })
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CommonText {
    label: CommonLabel,
    text: TextRef,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Choices {
    labels: [CommonLabel; 3],
    /// The fourth byte is stored alongside the three selectable entries.
    storage: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Prompt {
    Checking,
    ConfirmSave,
    ConfirmLoad,
    ConfirmOverwrite,
    CorruptFile,
    Saving,
    Loading,
    DeletingCorruptFile,
    SaveComplete,
    LoadComplete,
    DeleteComplete,
    FormatComplete,
    InsufficientSpace,
    #[serde(rename = "corrupt_card_format_prompt")]
    CorruptCardFormat,
    CorruptCard,
    ConfirmEraseAll,
    UnsupportedCard,
    DamagedCard,
    SaveFailedDamagedCard,
    WrongDevice,
    FormatFailed,
    SaveFailed,
    LoadFailed,
    DeleteFailed,
    Formatting,
}

const PROMPTS: [Prompt; 28] = {
    use Prompt::*;
    [
        Checking,
        ConfirmSave,
        ConfirmLoad,
        ConfirmOverwrite,
        CorruptFile,
        Saving,
        Loading,
        DeletingCorruptFile,
        SaveComplete,
        LoadComplete,
        DeleteComplete,
        FormatComplete,
        InsufficientSpace,
        InsufficientSpace,
        CorruptCardFormat,
        CorruptCard,
        CorruptCardFormat,
        CorruptCard,
        ConfirmEraseAll,
        UnsupportedCard,
        DamagedCard,
        SaveFailedDamagedCard,
        WrongDevice,
        FormatFailed,
        SaveFailed,
        LoadFailed,
        DeleteFailed,
        Formatting,
    ]
};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CardMessage {
    /// The pair is ordered by physical memory-card slots A and B.
    Slots {
        prompt: Prompt,
        texts: [TextRef; 2],
    },
    Accessing {
        text: TextRef,
    },
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Label {
    ExitSaveMenu,
    PlayTimeFormat,
    SaveBannerFormat,
    FatalError,
    EmptyMessage,
    StartupProgressMarkers,
    PartyPositionFormat,
    Level,
    UsagePercentFormat,
    Percent,
    SlotCountFormat,
    EmptySlotCount,
    ClearedMarker,
    Corrupt,
    ProbeFileName,
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<u32>)> {
    let mut texts = TextPool::default();
    let common = CommonLabel::ALL
        .into_iter()
        .zip(dol::slice(executable, COMMON, 41 * 4)?.chunks_exact(4))
        .map(|(label, pointer)| {
            Ok(CommonText {
                label,
                text: texts.required(executable, word(pointer, 0)?)?,
            })
        })
        .collect::<Result<_>>()?;
    let mut address = MESSAGES;
    let mut pointer = || -> Result<TextRef> {
        let value = texts.required(executable, word(dol::slice(executable, address, 4)?, 0)?)?;
        address += 4;
        Ok(value)
    };
    let mut card_messages = Vec::new();
    for (index, prompt) in PROMPTS.into_iter().enumerate() {
        if index == 12 {
            card_messages.push(CardMessage::Accessing { text: pointer()? });
        }
        card_messages.push(CardMessage::Slots {
            prompt,
            texts: [pointer()?, pointer()?],
        });
    }
    let choices = |address| -> Result<Choices> {
        let bytes = dol::slice(executable, address, 4)?;
        let label = |index: usize| {
            CommonLabel::ALL
                .get(usize::from(bytes[index]))
                .copied()
                .context("save-menu choice exceeds common labels")
        };
        Ok(Choices {
            labels: [label(0)?, label(1)?, label(2)?],
            storage: bytes[3],
        })
    };
    let mut labels = BTreeMap::new();
    for (label, address) in [
        (Label::ExitSaveMenu, 0x8019d2d4),
        (Label::PlayTimeFormat, 0x8019d2e4),
        (Label::SaveBannerFormat, 0x8019d2f0),
        (Label::FatalError, 0x8019d334),
        (Label::EmptyMessage, 0x8035ce28),
        (Label::StartupProgressMarkers, 0x8035ce34),
        (Label::PartyPositionFormat, 0x8035ce3c),
        (Label::Level, 0x8035ce40),
        (Label::UsagePercentFormat, 0x8035ce44),
        (Label::Percent, 0x8035ce4c),
        (Label::SlotCountFormat, 0x8035ce50),
        (Label::EmptySlotCount, 0x8035ce58),
        (Label::ClearedMarker, 0x8035ce5c),
        (Label::Corrupt, 0x8035ce60),
        (
            Label::ProbeFileName,
            word(dol::slice(executable, 0x8035cdbc, 4)?, 0)?,
        ),
    ] {
        labels.insert(label, texts.required(executable, address)?);
    }
    // Preserve the unresolved default without interpreting adjacent inline text as a pointer.
    let instruction = word(dol::slice(executable, 0x800b5c00, 4)?, 0)?;
    ensure!(
        instruction >> 16 == 0x8004,
        "unexpected save-menu default message lookup"
    );
    let byte_offset = instruction as i16;
    let target = COMMON
        .checked_add_signed(i32::from(byte_offset))
        .context("save-menu default offset overflow")?;
    Ok((
        Catalogue {
            texts: texts.values,
            common,
            card_messages,
            format_card_choices: choices(CHOICES)?,
            corrupt_file_choices: choices(CHOICES + 4)?,
            labels,
            default_error_reference: DefaultErrorReference {
                byte_offset,
                target_word: word(dol::slice(executable, target, 4)?, 0)?,
            },
        },
        texts
            .sources
            .into_iter()
            .map(|source| source.address)
            .collect(),
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; no media conversion or playback"]
    fn original_save_menu_preserves_pointer_aliases_choices_and_all_messages() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let mut executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let (catalogue, addresses) = parse(&executable)?;
            let encoded = serde_json::to_vec(&catalogue)?;
            let restored: Catalogue = serde_json::from_slice(&encoded)?;
            assert_eq!(restored, catalogue);
            if let Some(expected) = &first {
                assert_eq!(&encoded, expected);
            } else {
                first = Some(encoded);
            }
            let pointer_bytes = |references: Vec<TextRef>| {
                references
                    .into_iter()
                    .flat_map(|reference| addresses[reference.0].to_be_bytes())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                pointer_bytes(restored.common.iter().map(|row| row.text).collect()),
                dol::slice(&executable, COMMON, 164)?
            );
            let mut refs = Vec::new();
            for message in &restored.card_messages {
                match message {
                    CardMessage::Slots { texts, .. } => refs.extend(texts),
                    CardMessage::Accessing { text } => refs.push(*text),
                }
            }
            assert_eq!(pointer_bytes(refs), dol::slice(&executable, MESSAGES, 228)?);
            for (&address, text) in addresses.iter().zip(&restored.texts) {
                let terminated = [text.as_bytes(), &[0]].concat();
                assert_eq!(
                    terminated,
                    dol::slice(&executable, address, terminated.len())?
                );
            }
            let mut selectors = Vec::new();
            for choices in [
                &restored.format_card_choices,
                &restored.corrupt_file_choices,
            ] {
                selectors.extend(choices.labels.map(|label| label as u8));
                selectors.push(choices.storage);
            }
            assert_eq!(selectors, dol::slice(&executable, CHOICES, 8)?);
            assert_eq!(restored.common[13].text, restored.common[15].text);
            assert_eq!(restored.common[14].text, restored.common[30].text);
            assert_eq!(restored.default_error_reference.byte_offset, 164);
            assert_eq!(restored.default_error_reference.target_word, 0x43686563);

            // Retained bytes are not assumed zero; aliases and selectors follow edits.
            for (address, replacement) in [
                (CHOICES + 3, vec![0xa5]),
                (CHOICES + 7, vec![0x5a]),
                (CHOICES, vec![4]),
                (0x800b5c00, 0x8004fffcu32.to_be_bytes().to_vec()),
                (
                    COMMON + 15 * 4,
                    addresses[restored.common[5].text.0].to_be_bytes().to_vec(),
                ),
            ] {
                let source = dol::slice(&executable, address, replacement.len())?;
                let offset = source.as_ptr() as usize - executable.as_ptr() as usize;
                executable[offset..offset + replacement.len()].copy_from_slice(&replacement);
            }
            let changed = parse(&executable)?.0;
            assert_eq!(changed.default_error_reference.byte_offset, -4);
            assert_eq!(
                changed.default_error_reference.target_word,
                word(dol::slice(&executable, COMMON - 4, 4)?, 0)?
            );
            assert_eq!(changed.format_card_choices.storage, 0xa5);
            assert_eq!(changed.corrupt_file_choices.storage, 0x5a);
            assert_eq!(changed.format_card_choices.labels[0], CommonLabel::Yes);
            assert_eq!(changed.common[15].text, changed.common[5].text);
            assert_ne!(changed.common[13].text, changed.common[15].text);
        }
        Ok(())
    }
}
