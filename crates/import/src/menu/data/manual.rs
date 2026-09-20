use super::*;
use resonance_content::menu_data::{ManualChapter, ManualTopic, TrainingManual};

pub(super) fn cook(ui: &synopsis_catalogue::Catalogue) -> Result<TrainingManual> {
    let chapters = ui
        .manual
        .chapters
        .iter()
        .map(|chapter| {
            let count = chapter.topic_count;
            ensure!((1..=5).contains(&count), "invalid manual chapter length");
            Ok(ManualChapter {
                name: ui.required(chapter.name)?.to_owned(),
                topics: chapter.topics[..usize::from(count)]
                    .iter()
                    .map(|topic| {
                        Ok(ManualTopic {
                            name: ui.required(topic.name)?.to_owned(),
                            learned_flag: topic.learned_flag.into(),
                            paragraphs: topic
                                .paragraphs
                                .as_ref()
                                .context("null selected manual body")?
                                .iter()
                                .map(|&reference| super::text::decode(ui.text(reference), 9))
                                .collect::<Result<_>>()?,
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(TrainingManual {
        title: ui.required(ui.manual.title)?.to_owned(),
        chapters,
    })
}
