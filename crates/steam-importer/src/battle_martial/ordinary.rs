//! Proven ordinary native actions use the existing authored-action document.
use super::*;
use resonance_content::{
    battle_martial::{AuthoredAction, Presentation},
    battle_unison::Placement,
};

const ACTIONS: [(u16, u8, u16, &str); 6] = [
    (30, 1, 149, "battle/artes/sword-rain-tempest/action.json"),
    (153, 6, 151, "battle/artes/holy-binds-zelos/action.json"),
    (196, 8, 132, "battle/artes/dragon-strike/action.json"),
    (198, 8, 134, "battle/artes/heaven-rising/action.json"),
    (199, 8, 135, "battle/artes/dragons-talon/action.json"),
    (249, 9, 148, "battle/artes/holy-binds-kratos/action.json"),
];

pub(crate) fn action_path(image: &pe::Image<'_>, id: usize, row: &[u8]) -> Result<Option<String>> {
    if id == 228 {
        return super::falcon::action_path(image, row).map(Some);
    }
    let Some(&(_, _, member, path)) = ACTIONS
        .iter()
        .find(|&&(catalogue, ..)| usize::from(catalogue) == id)
    else {
        return Ok(None);
    };
    ensure!(
        pe::half(row, 0)? == member,
        "authored martial source binding changed for {id}"
    );
    if matches!(id, 30 | 196 | 198 | 199) {
        let descriptor = pe::word(image.slice(0x8e98d0 + u32::from(member) * 4, 4)?, 0)?;
        ensure!(
            pe::word(image.slice(descriptor, 4)?, 0)? == 0x427dc0
                && pe::word(image.slice(descriptor + 4, 4)?, 0)? == 0x427f20,
            "authored martial action {id} no longer uses the ordinary dispatcher"
        );
    }
    Ok(Some(path.to_owned()))
}

pub(super) fn cook(source: &Source, library: &Path, catalogue: &Catalogue) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    for (id, character, member, _) in ACTIONS {
        let row = image.slice(0x8d84b0 + u32::from(id) * 96, 96)?;
        let path =
            action_path(&image, usize::from(id), row)?.context("missing authored action path")?;
        let binding = *catalogue
            .bindings
            .get(&id)
            .and_then(|characters| characters.get(&character))
            .with_context(|| format!("missing authored martial binding {id}/{character}"))?;
        ensure!(
            (binding.member, binding.variant) == (member, 0),
            "authored martial binding changed for {id}"
        );
        let attack = catalogue.variant(binding)?.clone();
        if character == 8 {
            ensure!(
                attack
                    .events
                    .iter()
                    .any(|(_, event)| matches!(event, Event::Contact(_))),
                "Regal arte {id} has no native contact timeline"
            );
        }
        let presentation = if matches!(id, 153 | 249) {
            // These callbacks start their second stored program at raw ages4/34.
            // The native opening blend holds the action clock before that age.
            let blend = attack
                .events
                .iter()
                .find_map(|(at, event)| match event {
                    Event::Pose(pose) if *at == 0 => Some(u16::from(pose.blend)),
                    _ => None,
                })
                .context("Holy Binds has no opening pose")?;
            let second_at = (if id == 249 { 34u16 } else { 4u16 })
                .checked_add(blend)
                .context("Holy Binds effect time overflow")?;
            vec![
                (
                    0,
                    Presentation::SkillEffect {
                        member: 1,
                        bone: None,
                    },
                ),
                (
                    second_at,
                    Presentation::SkillEffect {
                        member: 2,
                        bone: None,
                    },
                ),
            ]
        } else {
            Vec::new()
        };
        output::json(
            &library.join(path),
            &AuthoredAction {
                attack,
                motions: BTreeMap::new(),
                effects: None,
                presentation,
                unison: Placement {
                    native_id: pe::half(row, 0)?,
                    altitude: pe::word(row, 0x50)?,
                    distance: pe::half(row, 0x54)?,
                    duration: pe::half(row, 0x56)?,
                },
            },
        )?;
    }
    Ok(())
}
