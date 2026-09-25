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
    cinematics: Vec<String>,
    dialogue: Vec<resonance_content::overworld::CinematicDialogue>,
}
impl Sources {
    pub(super) fn read(extracted: &Path, rel: &Rel, executable: &[u8]) -> Result<Self> {
        let files = extracted.join("files");
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
            dialogue: (0..9)
                .map(|index| {
                    let row = rel.at((6, index * 12))?;
                    let (duration, speaker) = [
                        (120, 1),
                        (120, 5),
                        (140, 4),
                        (240, 5),
                        (120, 5),
                        (80, 3),
                        (120, 5),
                        (60, 1),
                        (60, 3),
                    ][index];
                    Ok(resonance_content::overworld::CinematicDialogue {
                        tick: crate::read::u32(row, 0)?,
                        voice: crate::read::u32(row, 4)?,
                        text: rel.text(rel.pointer(6, index * 12 + 8)?)?,
                        duration,
                        speaker,
                    })
                })
                .collect::<Result<_>>()?,
            animation_members,
            skies: [
                crate::compression::payload(rel.at(rel.pointer(6, 0x2a56c)?)?.to_vec())?,
                crate::compression::payload(rel.at(rel.pointer(6, 0x2a570)?)?.to_vec())?,
            ],
        })
    }
    pub(super) fn prepare(&self, output: &Path, decoded: &Package) -> Result<Visuals> {
        let read = |path: &str| crate::compression::payload(decoded.source(path)?.as_ref().clone());
        let common = read(&self.common)?;
        ensure!(
            crate::field::sections(&common)?.len() == 21,
            "unexpected world actor archive"
        );
        let mounts = member(&common, 20)?.context("missing world mount motion bank")?;
        let mut visuals = Visuals::default();
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
            let count = [1, 1, 1, 9, 8, 8, 2, 2, 4, 7, 7, 7, 2, 2][index];
            let mut actors = BTreeMap::new();
            for index in 1..=count {
                let package = member(&archive, index)?.context("missing cinematic actor")?;
                actors.insert(
                    index as u8,
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
                    world: if matches!(id, 513..=516 | 522..=525) {
                        resonance_content::overworld::World::Sylvarant
                    } else {
                        resonance_content::overworld::World::TetheAlla
                    },
                    camera,
                    actors,
                    dialogue: if id == 517 {
                        self.dialogue.clone()
                    } else {
                        Vec::new()
                    },
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
