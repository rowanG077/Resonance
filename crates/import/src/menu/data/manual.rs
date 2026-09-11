use super::*;
use resonance_content::menu_data::{
    MANUAL_CHAPTERS, ManualChapter, ManualTopic, MenuText, TrainingManual,
};

pub(super) fn cook(
    executable: &[u8],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<TrainingManual> {
    let chapters = (0..MANUAL_CHAPTERS as u32)
        .map(|chapter| {
            let count = dol::slice(executable, 0x8019d808 + chapter, 1)?[0];
            ensure!((1..=5).contains(&count), "invalid manual chapter length");
            Ok(ManualChapter {
                name: text(dol::slice(executable, 0x8019d7e4 + chapter * 4, 4)?, 0)?,
                topics: (0..u32::from(count))
                    .map(|topic| {
                        let row = dol::slice(executable, 0x801a0bdc + chapter * 40 + topic * 8, 8)?;
                        Ok(ManualTopic {
                            name: text(row, 0)?,
                            learned_flag: dol::slice(
                                executable,
                                0x8019d814 + chapter * 5 + topic,
                                1,
                            )?[0]
                                .into(),
                            paragraphs: paragraphs(
                                executable,
                                u32::from_be_bytes(row[4..].try_into()?),
                            )?,
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(TrainingManual {
        title: text(dol::slice(executable, 0x8019d6f0, 4)?, 0)?,
        chapters,
    })
}

fn paragraphs(executable: &[u8], mut address: u32) -> Result<Vec<MenuText>> {
    let mut paragraphs = Vec::new();
    loop {
        ensure!(paragraphs.len() < 32, "unterminated manual topic");
        let (paragraph, next) = super::text::paragraph(executable, address)?;
        paragraphs.push(paragraph);
        if dol::slice(executable, next, 1)? == [0] {
            return Ok(paragraphs);
        }
        address = next;
    }
}
