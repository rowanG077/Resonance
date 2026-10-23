//! Runtime landmarks use full travel coordinates, independent of menu artwork.
use super::world_map as source;
use crate::{read, rel::Rel};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::overworld::{Interaction, Landmark, Landmarks, Marker, ShipCameraClearance};
use std::path::Path;

pub(crate) fn prepare(source: &source::Catalogue) -> Result<Landmarks> {
    let mut worlds: [Vec<Landmark>; 2] = Default::default();
    for (world, entries) in worlds.iter_mut().enumerate() {
        let rows = source.world(world)?;
        ensure!(
            rows.last().is_some_and(source::Location::is_terminator),
            "missing world location terminator"
        );
        for (index, row) in rows
            .iter()
            .enumerate()
            .skip(1)
            .take_while(|(_, row)| !row.is_terminator())
        {
            let name = source.required_text(row.text)?.to_owned();
            entries.push(Landmark {
                id: (world * 256 + index) as u16,
                position: row.position.map(|v| v as f32),
                height: (row.height != 0.).then_some(if row.height == 0.01 {
                    0.
                } else {
                    row.height
                }),
                radius: f32::from(row.radius),
                interaction: match row.interaction {
                    source::Interaction::Disabled => Interaction::Disabled,
                    source::Interaction::Active => Interaction::Active,
                    source::Interaction::Blocked => Interaction::Blocked,
                    source::Interaction::Unknown(v) => bail!("unknown world interaction {v}"),
                },
                marker: match row.marker {
                    source::Marker::None => Marker::None,
                    source::Marker::Model { id } => Marker::Model { id },
                    source::Marker::FieldPoint => Marker::FieldPoint,
                    source::Marker::Unmodeled => Marker::Unmodeled,
                    source::Marker::Unknown { value } => bail!("unknown world marker {value}"),
                },
                automatic: name.starts_with('*'),
                entry_overrides: Vec::new(),
                ship_camera_clearance: None,
                name,
            });
        }
    }
    let mut landmarks = Landmarks {
        worlds,
        item_rewards: Default::default(),
        party_requirements: Default::default(),
    };
    for row in source.item_rewards.iter().take_while(|r| r.location != 0) {
        ensure!(
            landmarks
                .item_rewards
                .insert(row.location, row.item)
                .is_none(),
            "duplicate world reward"
        );
    }
    for row in source
        .party_requirements
        .iter()
        .take_while(|r| r.location != 0)
    {
        ensure!(
            landmarks
                .party_requirements
                .insert(
                    row.location,
                    u8::try_from(row.required_character)
                        .context("invalid world party requirement")?
                )
                .is_none(),
            "duplicate world party requirement"
        );
    }
    landmarks.validate()?;
    Ok(landmarks)
}

pub(super) fn bind_camera_clearance(landmarks: &mut Landmarks, module: &Rel) -> Result<()> {
    let exire = landmarks.worlds[1]
        .iter_mut()
        .find(|landmark| landmark.id == 266)
        .context("Exire landmark is missing")?;
    let height = module.at((5, 0xb90))?;
    exire.ship_camera_clearance = Some(ShipCameraClearance {
        near_margin: read::f32(module.at((5, 0x954))?, 0)?,
        far_margin: read::f32(module.at((5, 0xb8c))?, 0)?,
        height: f64::from_be_bytes(
            height
                .get(..8)
                .context("truncated ship camera height")?
                .try_into()?,
        ) as f32,
    });
    landmarks.validate()
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(
        file,
        output,
        "overworld-landmarks",
        &prepare(&source::read(executable)?)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires both original executables"]
    fn original_overworld_landmarks_preserve_full_coordinates_and_handlers() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let file = root.join(format!("disc{disc}/sys/main.dol"));
            let bytes = std::fs::read(&file)?;
            let source = source::read(&bytes)?;
            let prepared = prepare(&source)?;
            assert_eq!(prepared.worlds.each_ref().map(Vec::len), [98, 81]);
            for (world, rows) in prepared.worlds.iter().enumerate() {
                for entry in rows {
                    let original = &source.world(world)?[usize::from(entry.id & 255)];
                    assert_eq!(entry.position, original.position.map(|v| v as f32));
                    assert_eq!(entry.name, source.required_text(original.text)?);
                }
            }
            assert_eq!(prepared.worlds[0][1].name, "Iselia");
            assert_eq!(prepared.worlds[1][47].id, 304);
            let out = tempfile::tempdir()?;
            cook(&file, &bytes, out.path())?;
            assert_eq!(
                crate::embedded::read::<Landmarks>(out.path(), "overworld-landmarks", "main.dol")?,
                prepared
            );
            let mut prepared = prepared;
            bind_camera_clearance(
                &mut prepared,
                &Rel::read(&root.join(format!("disc{disc}/files/US_r_Top2field.rel")))?,
            )?;
            assert_eq!(
                prepared.worlds[1][9].ship_camera_clearance,
                Some(ShipCameraClearance {
                    near_margin: 300.,
                    far_margin: 4250.,
                    height: 885.,
                })
            );
        }
        Ok(())
    }
}
