//! Shared chanting tracks installed by the party casting entrypoint.
use super::{
    action_program::{self, Command, Record},
    actions, animation_table, embedded,
};
use crate::{cooked::Source, read::Storage, rel::Rel};
use anyhow::{Context, Result};
use resonance_content::battle::actions::{AnimationProgram, TimedCommand};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub animation: usize,
    pub commands: usize,
    pub end: usize,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Programs {
    animation: animation_table::Parsed,
    commands: Vec<Command>,
    loop_commands: bool,
    command_storage: Storage,
}

impl Programs {
    fn read(rel: &Rel, layout: Layout) -> Result<Self> {
        let data = rel.at((5, 0))?;
        let animation = animation_table::decode(
            data.get(layout.animation..layout.commands)
                .context("invalid casting animation extent")?,
            [0],
        )?;
        let bytes = data
            .get(layout.commands..layout.end)
            .context("invalid casting command extent")?;
        let mut cursor = 0;
        let mut commands = Vec::new();
        let loop_commands = loop {
            match action_program::record(bytes, &mut cursor)? {
                Record::Command(command) => commands.push(command),
                Record::End { loops } => break loops,
            }
        };
        Ok(Self {
            animation,
            commands,
            loop_commands,
            command_storage: Storage {
                offset: cursor,
                bytes: bytes[cursor..].to_vec(),
            },
        })
    }

    pub fn bind(source: &Source<'_>) -> Result<Self> {
        source.embedded("battle-casting-programs", "US_r_Top2Btl.rel")
    }

    #[cfg(test)]
    pub fn original(rel: &Rel) -> Result<Self> {
        Self::read(rel, embedded::Layout::RETAIL.casting_programs)
    }

    pub fn animation(&self) -> Result<AnimationProgram> {
        self.animation.selected(0)
    }

    pub fn commands(&self) -> Result<(Vec<TimedCommand>, bool)> {
        Ok((
            self.commands
                .iter()
                .map(actions::lower_command)
                .collect::<Result<_>>()?,
            self.loop_commands,
        ))
    }
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = embedded::Layout::identify(file) else {
        return Ok(None);
    };
    let layout = layout.casting_programs;
    let programs = Programs::read(&Rel::read(file)?, layout)?;
    embedded::write(
        file,
        output,
        "battle-casting-programs",
        &programs,
        serde_json::json!({
            "section":5,
            "animation":{"offset":layout.animation, "end":layout.commands},
            "commands":{"offset":layout.commands, "end":layout.end},
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires original battle modules; no media conversion"]
    fn original_casting_programs_bind_all_module_layouts() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("casting-programs"));
        let mut publications = BTreeSet::new();
        for disc in ["disc1", "disc2"] {
            let files = extracted.join(disc).join("files");
            let original = Rel::read(&files.join("US_r_Top2Btl.rel"))?;
            let animation = actions::animations_at(original.at((5, 0))?, 0x11e0)?;
            let commands = actions::commands(original.at((5, 0x1210))?)?;
            for module in [
                "US_r_Top2Btl.rel",
                "r_Top2Btl.rel",
                "US_Top2Btl.rel",
                "US_m_Top2Btl.rel",
                "Top2Btl.rel",
                "m_Top2Btl.rel",
                "Top2BtlD.rel",
            ] {
                let path = files.join(module);
                let rel = Rel::read(&path)?;
                let (_, layout) = embedded::Layout::identify(&path).unwrap();
                for root in [
                    layout.casting_programs.animation,
                    layout.casting_programs.commands,
                ] {
                    assert!(rel.local_targets().contains(&(5, root)));
                }
                let paths = cook(&path, &output)?.unwrap();
                publications.insert(paths[0].clone());
                let programs: Programs =
                    crate::embedded::read(&output, "battle-casting-programs", module)?;
                assert_eq!(
                    serde_json::to_vec(&programs.animation()?)?,
                    serde_json::to_vec(&animation)?
                );
                assert_eq!(
                    serde_json::to_vec(&programs.commands()?)?,
                    serde_json::to_vec(&commands)?
                );
                assert_eq!(programs.command_storage.offset, 34);
                assert_eq!(programs.command_storage.bytes, [0; 6]);
            }
        }
        assert_eq!(publications.len(), 1);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
