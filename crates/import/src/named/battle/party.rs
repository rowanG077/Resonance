use super::*;
use resonance_content::{
    battle_model::{self, document as models},
    battle_victory::{self, document as victory},
};

impl Publisher<'_> {
    pub fn battle_party(
        &mut self,
        parties: impl IntoIterator<Item = ((u8, u8), battle_model::Party)>,
    ) -> Result<()> {
        let mut characters = BTreeMap::new();
        let mut costumes = BTreeMap::<String, BTreeMap<u32, String>>::new();
        for ((character, costume), mut party) in parties {
            ensure!(
                (1..=9).contains(&character),
                "invalid party character {character}"
            );
            let name = party_owner(u32::from(character))
                .unwrap()
                .strip_prefix("meshes/characters/")
                .unwrap()
                .to_owned();
            let variant = if costume == 0 {
                "default".to_owned()
            } else {
                format!("costume-{costume}")
            };
            let owner = format!("battle/party/{name}/{variant}");
            let entries = costumes.entry(name.clone()).or_default();
            ensure!(
                entries.insert(u32::from(costume), variant).is_none(),
                "duplicate party costume {character}:{costume}"
            );
            characters.insert(u32::from(character), name.clone());
            let body = party_owner(0x2300_0000 | (u32::from(costume) << 8) | u32::from(character))
                .unwrap();
            for (index, part) in party.parts.iter_mut().enumerate() {
                self.body(&body, index, part)?;
                self.clips(&format!("{owner}/animations"), index, &mut part.clips)?;
            }
            let model = format!("{owner}/model.json");
            self.json(
                &model,
                &models::Visuals {
                    version: models::VERSION,
                    parts: party.parts,
                },
            )?;
            let rig = format!("{owner}/rig.json");
            self.json(&rig, &party.body)?;
            let wings = party
                .wings
                .map(|mut part| -> Result<_> {
                    let wing_owner = format!("battle/party/{name}/wings");
                    self.body(&format!("meshes/characters/{name}/wings"), 0, &mut part)?;
                    self.clips(&format!("{wing_owner}/animations"), 0, &mut part.clips)?;
                    let path = format!("{wing_owner}/model.json");
                    self.json(
                        &path,
                        &models::Visuals {
                            version: models::VERSION,
                            parts: vec![part],
                        },
                    )?;
                    Ok(path)
                })
                .transpose()?;
            let victory = format!("{owner}/victory.json");
            self.json(
                &format!("{owner}/party.json"),
                &models::PartyDefinition {
                    version: models::VERSION,
                    rig,
                    model,
                    null_motions: party.null_motions,
                    wings,
                    victory,
                },
            )?;
        }
        for (name, costumes) in costumes {
            self.json(
                &format!("battle/party/{name}/mappings.json"),
                &Mappings::new(costumes)?,
            )?;
        }
        self.json(models::PARTY, &Mappings::new(characters)?)
    }

    /// Native preparation supplies character/costume identity directly. Equal
    /// body bytes do not merge the independent editable victory owners.
    pub fn battle_victories(
        &mut self,
        performances: &battle_victory::Performances,
        root: &str,
    ) -> Result<Vec<String>> {
        library::validate_path(root)?;
        let mut selections = BTreeMap::<(u8, u8), BTreeMap<u8, victory::Performance>>::new();
        for row in &performances.ordinary {
            let costume = row.costume.context("victory has no costume owner")?;
            let relative = party_directory(row.character, costume)?;
            let owner = format!("{root}/party/{relative}");
            ensure!(row.selector < 5, "unsupported victory selector");
            let path = format!("{owner}/victory/performance-{}.motion.json", row.selector);
            let motion = resonance_content::animation::Motion::decode(&fs::read(
                self.source.join(&row.motion),
            )?)?;
            self.write(&path, &motion.encode()?)?;
            ensure!(
                selections
                    .entry((row.character, costume))
                    .or_default()
                    .insert(
                        row.selector,
                        victory::Performance {
                            motion: path,
                            timeline: row.timeline.clone(),
                            unsupported_reason: row.unsupported_reason.clone(),
                        },
                    )
                    .is_none(),
                "duplicate victory selector for {}:{costume}",
                row.character
            );
        }
        let mut paths = Vec::new();
        for ((character, costume), performances) in selections {
            ensure!(
                performances.len() == 5,
                "incomplete original victory owner {character}:{costume}"
            );
            let path = format!(
                "{root}/party/{}/victory.json",
                party_directory(character, costume)?
            );
            self.json(
                &path,
                &victory::Definition {
                    version: victory::VERSION,
                    performances,
                },
            )?;
            paths.push(path);
        }
        let groups = format!("{root}/victory/groups.json");
        self.json(
            &groups,
            &victory::Groups {
                version: victory::VERSION,
                groups: performances.groups.clone(),
            },
        )?;
        paths.push(groups);
        Ok(paths)
    }
}

fn party_directory(character: u8, costume: u8) -> Result<String> {
    let body = party_owner(u32::from(character)).context("invalid party character")?;
    let name = body.strip_prefix("meshes/characters/").unwrap();
    let variant = if costume == 0 {
        "default".into()
    } else {
        format!("costume-{costume}")
    };
    Ok(format!("{name}/{variant}"))
}

pub(crate) fn party_path(character: u8, costume: u8) -> Result<String> {
    Ok(format!(
        "battle/party/{}/party.json",
        party_directory(character, costume)?
    ))
}
