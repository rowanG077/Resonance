use super::*;
use resonance_content::battle_event::{self, document as events};

impl Publisher<'_> {
    pub fn battle_events(
        &mut self,
        scenes: impl IntoIterator<Item = (u16, battle_event::Scene)>,
    ) -> Result<Vec<String>> {
        let mut mappings = BTreeMap::new();
        let mut paths = Vec::new();
        for (id, scene) in scenes {
            let blink = scene.blink.context("event has no authored blink cycle")?;
            blink.validate()?;
            self.json(events::BLINK, &blink)?;
            ensure!(
                !mappings.contains_key(&u32::from(id)),
                "duplicate battle event ID {id}"
            );
            let name = match id {
                2 => "vidarr-opening".into(),
                3 => "vidarr-rescue".into(),
                _ => format!("event-{id:04}"),
            };
            let owner = format!("battle/events/{name}");
            let script = format!("{owner}/scenario.ssb");
            let messages = format!("{owner}/messages.json");
            self.copy(&scene.script, &script)?;
            self.copy(&scene.messages, &messages)?;
            let mut actors = BTreeMap::new();
            for (resource, mut model) in scene.actors {
                // These original event bindings select default party bodies and
                // Vidarr explicitly. Equal bytes on another actor never alias.
                let party = matches!(id, 2 | 3).then(|| party_owner(resource)).flatten();
                let shared = if let Some(owner) = &party {
                    owner.clone()
                } else if matches!(id, 2 | 3) && resource == 17 {
                    format!(
                        "meshes/monsters/{}",
                        crate::naming::catalogue_model("monsters", 219)?
                    )
                } else if matches!(id, 2 | 3) && resource == 0x2005d {
                    "battle/events/models/soldier-0002005d".into()
                } else {
                    format!("{owner}/actors/actor-{resource:08x}")
                };
                let actor_name = if let Some(owner) = &party {
                    owner.strip_prefix("meshes/characters/").unwrap().to_owned()
                } else if matches!(id, 2 | 3) && resource == 17 {
                    "vidarr".into()
                } else if matches!(id, 2 | 3) && resource == 0x2005d {
                    "soldier".into()
                } else {
                    format!("actor-{resource:08x}")
                };
                let actor = format!("{owner}/actors/{actor_name}");
                for (index, part) in model.parts.iter_mut().enumerate() {
                    self.body(&shared, index, part)?;
                    if party.is_some() {
                        let (mut body, mut external): (Vec<_>, Vec<_>) = part
                            .clips
                            .drain(..)
                            .partition(|clip| clip.animation_resource.is_none());
                        self.clips(
                            &format!("battle/party/{actor_name}/default/animations"),
                            index,
                            &mut body,
                        )?;
                        self.clips(
                            &format!("battle/events/models/{actor_name}/animations"),
                            index,
                            &mut external,
                        )?;
                        // Original event resources precede the ordinary battle
                        // bank. Preserve order as well as scoped slot identity.
                        external.extend(body);
                        part.clips = external;
                    } else {
                        self.clips(&format!("{shared}/animations"), index, &mut part.clips)?;
                    }
                }
                let path = format!("{actor}/model.json");
                self.json(
                    &path,
                    &events::Actor {
                        version: events::VERSION,
                        skeleton: model.skeleton,
                        parts: model.parts,
                    },
                )?;
                actors.insert(resource, path);
            }
            let path = format!("{owner}/event.json");
            self.json(
                &path,
                &events::Definition {
                    version: events::VERSION,
                    script,
                    messages,
                    blink: events::BLINK.into(),
                    audio: scene.audio,
                    actors,
                },
            )?;
            paths.push(path);
            mappings.insert(u32::from(id), name);
        }
        self.json(events::MAPPINGS, &Mappings::new(mappings)?)?;
        paths.push(events::MAPPINGS.into());
        Ok(paths)
    }
}
