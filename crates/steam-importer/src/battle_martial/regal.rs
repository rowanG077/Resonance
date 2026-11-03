//! Added Regal rows use the native ordinary dispatcher and shared action format.
use super::*;
use resonance_content::{battle_martial::AuthoredAction, battle_unison::Placement};

const ACTIONS: [(u16, u16, &str); 3] = [
    (196, 132, "battle/artes/dragon-strike/action.json"),
    (198, 134, "battle/artes/heaven-rising/action.json"),
    (199, 135, "battle/artes/dragons-talon/action.json"),
];

pub(super) fn action_path(image: &pe::Image<'_>, id: usize, row: &[u8]) -> Result<Option<String>> {
    let Some(&(_, member, path)) = ACTIONS
        .iter()
        .find(|&&(catalogue, _, _)| usize::from(catalogue) == id)
    else {
        return Ok(None);
    };
    ensure!(
        pe::half(row, 0)? == member,
        "Regal arte {id} source binding changed"
    );
    let descriptor = pe::word(image.slice(0x8e98d0 + u32::from(member) * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == 0x427dc0
            && pe::word(image.slice(descriptor + 4, 4)?, 0)? == 0x427f20,
        "Regal arte {id} no longer uses the ordinary action dispatcher"
    );
    Ok(Some(path.to_owned()))
}

pub(super) fn cook(source: &Source, library: &Path, catalogue: &Catalogue) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    for (id, member, _) in ACTIONS {
        let row = image.slice(0x8d84b0 + u32::from(id) * 96, 96)?;
        let path =
            action_path(&image, usize::from(id), row)?.context("missing Regal authored path")?;
        let binding = catalogue
            .bindings
            .get(&id)
            .and_then(|rows| rows.get(&8))
            .context("missing Regal source binding")?;
        ensure!(
            (binding.member, binding.variant) == (member, 0),
            "Regal arte {id} variant changed"
        );
        let attack = catalogue.variant(*binding)?.clone();
        ensure!(
            attack
                .events
                .iter()
                .any(|(_, event)| matches!(event, Event::Contact(_))),
            "Regal arte {id} has no native contact timeline"
        );
        output::json(
            &library.join(path),
            &AuthoredAction {
                attack,
                motions: BTreeMap::new(),
                effects: None,
                presentation: Vec::new(),
                unison: Placement {
                    native_id: member,
                    altitude: pe::word(row, 0x50)?,
                    distance: pe::half(row, 0x54)?,
                    duration: pe::half(row, 0x56)?,
                },
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires native Steam; publishes Regal actions and current technique rules only"]
    fn installed_added_regal_actions_keep_native_ordinary_tracks() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let temporary = tempfile::tempdir()?;
        let root = std::env::var_os("RESONANCE_TEST_LIBRARY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| temporary.path().to_owned());
        let catalogue = super::super::read(&source)?;
        cook(&source, &root, &catalogue)?;
        crate::techniques::cook(&source, &root)?;
        let document: resonance_content::arte::document::Document =
            serde_json::from_slice(&std::fs::read(root.join(resonance_content::arte::PATH))?)?;
        for (id, member, path) in ACTIONS {
            let entry = &document.techniques[document.mappings.target(u32::from(id))?];
            ensure!(
                entry.rules.action.as_deref() == Some(path),
                "Regal action was not admitted"
            );
            let action: AuthoredAction = serde_json::from_slice(&std::fs::read(root.join(path))?)?;
            ensure!(
                action.unison.native_id == member,
                "Regal native identity changed"
            );
            ensure!(
                serde_json::to_value(&action.attack)?
                    == serde_json::to_value(catalogue.attack(id, 8)?)?,
                "Regal action lost native gameplay events"
            );
        }
        let finisher = catalogue.attack(200, 8)?;
        ensure!(
            finisher
                .events
                .iter()
                .any(|(_, event)| matches!(event, Event::AnimationRate(rate) if *rate == 0.005)),
            "Fanged Finality lost native slow motion"
        );
        Ok(())
    }
}
