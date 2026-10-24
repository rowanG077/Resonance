//! Native stored choreography, including unreferenced physical skill packages.
use super::*;

#[derive(Clone, Default)]
pub struct Publication {
    pub paths: Vec<String>,
    pub failures: BTreeMap<String, String>,
}

#[derive(Default)]
pub struct PreparedCatalogue {
    pub techniques: BTreeMap<u16, Prepared>,
    pub skills: BTreeMap<u16, Prepared>,
    pub unreferenced_skills: BTreeMap<u16, Prepared>,
    pub failures: BTreeMap<String, String>,
}

pub fn prepare_all(extracted: &Path, output: &Path) -> Result<PreparedCatalogue> {
    let sources = Sources::read(extracted)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let mut shared = crate::battle_effect::SharedPalettes::prepare(&usual, output)?;
    prepare_all_source(extracted, &sources, output, &mut shared)
}

pub(crate) fn prepare_all_source(
    extracted: &Path,
    sources: &Sources,
    output: &Path,
    shared: &mut crate::battle_effect::SharedPalettes,
) -> Result<PreparedCatalogue> {
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let element_sounds = crate::battle_profile::element_contact_sounds(&module)?;
    let mut result = PreparedCatalogue::default();
    let magic = extracted.join("files").join(&sources.magic);
    for (id, range) in super::magic_ranges(&module, &magic)? {
        let prepared = (|| -> Result<_> {
            let bytes = read_range(&magic, range)?;
            let mut prepared = super::read(&bytes, output, Some(id), &element_sounds, shared)?;
            super::scene_settings(&module, id, &mut prepared.scene)?;
            super::scene_actions(&bytes, id, &element_sounds, &mut prepared.scene)?;
            Ok(prepared)
        })();
        match prepared {
            Ok(prepared) => {
                result.techniques.insert(id, prepared);
            }
            Err(error) => {
                result
                    .failures
                    .insert(format!("technique-{id}"), format!("{error:#}"));
            }
        }
    }
    let skill = extracted.join("files").join(&sources.skill);
    let selectors = crate::arte::read(&fs::read(extracted.join("sys/main.dol"))?)?.skill_packages;
    let ranges = super::skill_ranges(&module, &skill)?;
    ensure!(
        selectors
            .values()
            .all(|id| ranges.iter().any(|(slot, _)| slot == id)),
        "skill selector has no physical package"
    );
    for (slot, range) in ranges {
        let prepared = (|| -> Result<_> {
            let bytes = read_range(&skill, range)?;
            super::read(&bytes, output, None, &element_sounds, shared)
        })();
        match prepared {
            Ok(prepared) => {
                let ids = selectors
                    .iter()
                    .filter_map(|(&id, &package)| (package == slot).then_some(id))
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    result.unreferenced_skills.insert(slot, prepared);
                } else {
                    for id in ids {
                        result.skills.insert(id, prepared.clone());
                    }
                }
            }
            Err(error) => {
                result
                    .failures
                    .insert(format!("skill-package-{slot}"), format!("{error:#}"));
            }
        }
    }
    Ok(result)
}
