//! Native world actor/landmark declarations bound to ordinary scene parts.
use crate::{read::u16 as half, rel::Rel, scene::decoded::Package};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::Visuals;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone)]
pub(super) struct Sources {
    pub dependencies: BTreeMap<String, String>,
    common: String,
    markers: [String; 2],
    party: Vec<[String; 2]>,
    animation_members: [usize; 4],
    skies: [Vec<u8>; 2],
    dust: Vec<u8>,
    cinematics: Vec<String>,
    cinematic_rules: BTreeMap<u16, super::overworld_cinematics::Definition>,
    cinematic_sounds: Vec<i16>,
}
impl Sources {
    pub(super) fn read(extracted: &Path, rel: &Rel, executable: &[u8]) -> Result<Self> {
        let files = extracted.join("files");
        let (cinematic_rules, cinematic_sounds) = super::overworld_cinematics::read(rel)?;
        let resolve =
            |path: &str| crate::field_resources::resolve_path(&files, path.trim_start_matches('/'));
        let common = resolve(&rel.text((5, 0x930))?)?;
        let markers = [
            resolve(&rel.text(rel.pointer(5, 0x888)?)?)?,
            resolve(&rel.text(rel.pointer(5, 0x88c)?)?)?,
        ];
        let catalogue = crate::resource::read(executable)?;
        let party = (1..=9)
            .map(|id| {
                Ok([
                    resolve(catalogue.party(crate::resource::PartyResource::Body, id, 0)?)?,
                    resolve(catalogue.field_motion(id)?)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        let cinematics = (0..14)
            .map(|index| resolve(&format!("FIELD/e{index:02}.d")))
            .collect::<Result<Vec<_>>>()?;
        let mut dependencies = BTreeMap::new();
        for path in std::iter::once(&common)
            .chain(&markers)
            .chain(party.iter().flatten())
            .chain(&cinematics)
        {
            dependencies.insert(crate::media::hash_file(&files.join(path))?, path.clone());
        }
        let animation_members = (0..4)
            .map(|index| {
                Ok(usize::from(
                    half(rel.at((5, 0x168))?, index * 2)?
                        .checked_sub(1)
                        .context("invalid world motion index")?,
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap();
        Ok(Self {
            dependencies,
            common,
            markers,
            party,
            cinematics,
            cinematic_rules,
            cinematic_sounds,
            animation_members,
            dust: rel.at((6, 0x2f100))?.to_vec(),
            skies: [
                crate::compression::payload(rel.at(rel.pointer(6, 0x2a56c)?)?.to_vec())?,
                crate::compression::payload(rel.at(rel.pointer(6, 0x2a570)?)?.to_vec())?,
            ],
        })
    }
    pub(super) fn cinematic_sounds(&self) -> &[i16] {
        &self.cinematic_sounds
    }
    pub(super) fn cinematic_voices(
        &self,
    ) -> impl Iterator<Item = &crate::media::field_audio::WorldVoiceArchive> {
        self.cinematic_rules
            .values()
            .filter_map(|scene| scene.voices.as_ref())
    }
    pub(super) fn prepare(&self, output: &Path, decoded: &Package) -> Result<Visuals> {
        let read = |path: &str| crate::compression::payload(decoded.source(path)?.as_ref().clone());
        let common = read(&self.common)?;
        ensure!(
            crate::field::sections(&common)?.len() == 21,
            "unexpected world actor archive"
        );
        let mounts = member(&common, 20)?.context("missing world mount motion bank")?;
        let mut visuals = Visuals {
            dust: crate::texture::decode_source(&self.dust)?
                .write(output)?
                .textures
                .into_iter()
                .map(|texture| Ok(texture.context("missing world dust sprite")?.image(0)?.path))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .map_err(|_| anyhow::anyhow!("expected four world dust sprites"))?,
            ..Default::default()
        };
        for (index, bytes) in self.skies.iter().enumerate() {
            let decoded_sky = Package::cook(
                bytes,
                &format!("assets/{}", crate::digest(bytes)),
                output,
                super::geometry::Input::File,
            )?;
            let mut sky = parts(output, "world sky", bytes, &[], &decoded_sky)?;
            for part in &mut sky {
                for material in &mut part.materials {
                    material.depth_write = false;
                }
            }
            visuals.skies[index] = sky;
        }

        for (index, paths) in self.party.iter().enumerate() {
            let model = read(&paths[0])?;
            let motions = read(&paths[1])?;
            let mut clips = self.clips(&motions, decoded)?;
            for slot in 0..2 {
                clips.push((
                    (4 + slot) as u16,
                    decoded.animation(
                        member(mounts, index * 2 + slot)?.context("missing party mount motion")?,
                    )?,
                ));
            }
            visuals.actors.insert(
                (index + 1) as u16,
                parts(
                    output,
                    &format!("world party {}", index + 1),
                    &model,
                    &clips,
                    decoded,
                )?,
            );
        }
        for (index, id) in [
            (0, 100),
            (1, 101),
            (2, 200),
            (3, 202),
            (4, 201),
            (5, 99),
            (6, 203),
            (7, 204),
            (8, 205),
            (9, 206),
            (10, 207),
            (11, 208),
            (12, 209),
            (13, 210),
            (14, 211),
            (15, 212),
            (16, 213),
            (17, 214),
            (18, 215),
            (19, 216),
        ] {
            let package = member(&common, index)?.context("missing world actor")?;
            let direct = crate::all_assets::geometry::is_model(package);
            let clips = if direct || id == 200 {
                Vec::new()
            } else {
                self.clips(package, decoded)?
            };
            let model = if matches!(id, 200 | 201) {
                member(package, 0)?.context("missing vehicle body")?
            } else {
                package
            };
            visuals.actors.insert(
                id,
                parts(output, &format!("world actor {id}"), model, &clips, decoded)?,
            );
        }
        for (world, path) in self.markers.iter().enumerate() {
            let archive = read(path)?;
            ensure!(
                crate::field::sections(&archive)?.len() == 17,
                "unexpected world landmark archive"
            );
            for index in 0..17 {
                let Some(model) = member(&archive, index)? else {
                    continue;
                };
                let clips = if index == 0 {
                    vec![(
                        0,
                        decoded
                            .animation(member(model, 2)?.context("missing landmark animation")?)?,
                    )]
                } else {
                    Vec::new()
                };
                let model = if index == 0 {
                    member(model, 0)?.context("missing animated landmark model")?
                } else {
                    model
                };
                visuals.markers[world].insert(
                    index as u8 + 1,
                    parts(
                        output,
                        &format!("world {world} landmark {}", index + 1),
                        model,
                        &clips,
                        decoded,
                    )?,
                );
            }
        }
        for (index, path) in self.cinematics.iter().enumerate() {
            let archive = read(path)?;
            let id = 513 + index as u16;
            let camera = crate::scene::camera(super::field::camera(
                member(&archive, 0)?.context("missing cinematic camera")?,
            )?)?;
            // These slots are actors. The e04 member 9 is
            // additional media; it is not an actor created by that scene.
            let rule = &self.cinematic_rules[&id];
            let mut actors = BTreeMap::new();
            for &index in rule.presentation.actors.keys() {
                let package =
                    member(&archive, usize::from(index))?.context("missing cinematic actor")?;
                actors.insert(
                    index,
                    parts(
                        output,
                        &format!("world cinematic {id} actor {index}"),
                        package,
                        &self.clips(package, decoded)?,
                        decoded,
                    )?,
                );
            }
            visuals.cinematics.insert(
                id,
                resonance_content::overworld::Cinematic {
                    world: rule.world,
                    presentation: rule.presentation.clone(),
                    camera,
                    actors,
                    dialogue: rule.dialogue.clone(),
                },
            );
        }
        Ok(visuals)
    }
    fn clips(
        &self,
        package: &[u8],
        decoded: &Package,
    ) -> Result<Vec<(u16, std::sync::Arc<crate::animation::AuthoredAnimation>)>> {
        self.animation_members
            .iter()
            .enumerate()
            .filter_map(|(slot, &index)| match member(package, index) {
                Ok(Some(bytes)) => Some(
                    decoded
                        .animation(bytes)
                        .map(|animation| (slot as u16, animation)),
                ),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }
}

fn member(package: &[u8], index: usize) -> Result<Option<&[u8]>> {
    let sections = crate::field::sections(package)?;
    Ok(sections
        .get(index)
        .and_then(|section| section.as_ref())
        .map(|range| &package[range.clone()]))
}
fn parts(
    output: &Path,
    label: &str,
    model: &[u8],
    clips: &[(u16, std::sync::Arc<crate::animation::AuthoredAnimation>)],
    decoded: &Package,
) -> Result<Vec<resonance_content::ScenePart>> {
    let clips = clips
        .iter()
        .map(|(slot, animation)| crate::character::Clip {
            slot: *slot,
            resource: None,
            animation,
        })
        .collect::<Vec<_>>();
    crate::character::cook_source_parts(output, label, model, &clips, decoded)
}
