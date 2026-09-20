//! Assemble costume and carried-model selections from the complete physical library.
use super::{Archive, Asset, Directory, Sources, Visual};
use crate::battle::visual::{costumes, weapon_owners};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::{
        unison::PowWeapon,
        visual::{PartyVariant, PartyVisuals, WeaponVisuals},
    },
    menu_data::Title,
    session::SessionData,
};
use std::{collections::BTreeMap, path::Path};

pub(in crate::battle::visual) fn party_metadata(
    root: &Path,
    disc: u8,
) -> Result<(Vec<Vec<Title>>, Vec<u16>)> {
    #[derive(serde::Deserialize)]
    struct Tables {
        titles: Vec<Vec<Title>>,
    }
    let source = crate::cooked::Source::open(root, disc, "sys/main.dol")?;
    let tables: Tables = source.document("embedded/menu/tables.json")?;
    ensure!(
        tables.titles.len() == 9
            && tables
                .titles
                .iter()
                .all(|titles| (1..32).contains(&titles.len())),
        "invalid cooked party titles"
    );
    let session: SessionData = source.document("game/session-data.json")?;
    session.validate()?;
    Ok((
        tables.titles,
        session
            .items
            .into_iter()
            .map(|item| item.allowed_characters)
            .collect(),
    ))
}

pub(in crate::battle::visual) fn weapons(
    root: &Path,
    disc: u8,
    sources: &Sources,
    ids: &[u16],
) -> Result<BTreeMap<u16, WeaponVisuals>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let directory = Directory::open(root, disc, sources.archive(Archive::Weapon))?;
    ids.iter()
        .map(|&id| {
            let Visual::Weapon(weapon) =
                directory.read(Asset::Weapon(id), &format!("weapon-{id}"))?
            else {
                anyhow::bail!("cooked weapon {id} contains another visual kind");
            };
            Ok((id, weapon))
        })
        .collect()
}

pub(in crate::battle::visual) fn pow_weapons(
    root: &Path,
    disc: u8,
    sources: &Sources,
    party: &[u8],
) -> Result<BTreeMap<PowWeapon, WeaponVisuals>> {
    let ids: Vec<_> = PowWeapon::ALL
        .into_iter()
        .filter(|kind| kind.selected(party))
        .collect();
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let directory = Directory::open(root, disc, sources.archive(Archive::Magic))?;
    ids.into_iter()
        .map(|kind| {
            let Visual::Weapon(weapon) = directory.read(
                Asset::PowWeapon(kind),
                &format!("pow-weapon-{}", kind.native()),
            )?
            else {
                anyhow::bail!("cooked Pow weapon {kind:?} contains another visual kind");
            };
            Ok((kind, weapon))
        })
        .collect()
}

pub(in crate::battle::visual) fn party(
    root: &Path,
    disc: u8,
    sources: &Sources,
    characters: &[u8],
    titles: &[Vec<Title>],
    weapons: &BTreeMap<u16, WeaponVisuals>,
    owners: &[u16],
) -> Result<BTreeMap<u8, PartyVisuals>> {
    if characters.is_empty() {
        return Ok(BTreeMap::new());
    }
    // Party jobs are also indexed under the common battle archive.
    let directory = Directory::open(root, disc, &sources.usual)?;
    let weapon_directory = Directory::open(root, disc, sources.archive(Archive::Weapon))?;
    let mut party = BTreeMap::new();
    for &character in characters {
        ensure!(
            (1..=9).contains(&character),
            "invalid party character {character}"
        );
        let mut pairs = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        let titles = titles
            .get(usize::from(character - 1))
            .context("missing party titles")?;
        for costume in costumes::required(character, titles) {
            let Visual::Party(mut model) = directory.read(
                Asset::Party { character, costume },
                &format!("party-{character}-costume-{}", costume as u8),
            )?
            else {
                anyhow::bail!(
                    "cooked party {character} costume {costume:?} contains another visual kind"
                );
            };
            ensure!(
                model.weapon_motions.is_empty(),
                "physical party model already has selected weapon motions"
            );
            let pair = (
                model.visual.model_sha256.clone(),
                model.visual.animation_sha256.clone(),
            );
            let binding = if let Some(&canonical) = pairs.get(&pair) {
                PartyVariant::Alias(canonical)
            } else {
                for (&item, weapon) in weapons {
                    if weapon_owners(owners, item) & (1 << (character - 1)) == 0 {
                        continue;
                    }
                    ensure!(
                        weapon.rigs.keys().all(|slot| model
                            .visual
                            .rig
                            .weapon_bones
                            .contains_key(slot)),
                        "party {character} costume {costume:?} lacks weapon {item} attachment slots"
                    );
                    if character == 3 {
                        let link = weapon
                            .motion_link
                            .as_ref()
                            .context("missing linked weapon motion bank")?;
                        ensure!(
                            link.owner == character,
                            "weapon {item} has a different linked motion owner"
                        );
                        let Visual::WeaponMotions(motions) = weapon_directory.read(
                            Asset::WeaponMotions { item, costume },
                            &format!("weapon-{item}-costume-{}", costume as u8),
                        )?
                        else {
                            anyhow::bail!(
                                "cooked weapon {item} costume {costume:?} contains another visual kind"
                            );
                        };
                        ensure!(
                            motions.rigs.keys().eq(weapon.rigs.keys()),
                            "costume weapon instances differ from prepared geometry"
                        );
                        model.weapon_motions.insert(item, motions);
                    }
                }
                pairs.insert(pair, costume);
                PartyVariant::Model(Box::new(model))
            };
            bindings.insert(costume, binding);
        }
        party.insert(character, PartyVisuals { bindings });
    }
    Ok(party)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::visual::{pow_blade, rel_data};
    use resonance_content::{battle::visual::VisualAssets, menu_data::Costume};
    use std::fs;

    #[test]
    #[ignore = "requires cook-all party visuals and original tables; no conversion or codecs"]
    fn original_cooked_party_library_binds_all_costumes_weapons_and_pow_on_both_discs() -> Result<()>
    {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = local.join("all-assets");
        let characters: Vec<_> = (1..=9).collect();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let sources = Sources::cooked(&root, disc)?;
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let rel = fs::read(extracted.join("files/US_r_Top2Btl.rel"))?;
            let offsets = (0..158)
                .map(|id| crate::read::u32(rel_data(&rel)?, 0x6e8 + id * 4))
                .collect::<Result<Vec<_>>>()?;
            let ids = crate::battle::all::physical_ranges(
                &offsets,
                0,
                extracted
                    .join("files")
                    .join(sources.archive(Archive::Weapon))
                    .metadata()?
                    .len(),
            )?
            .into_iter()
            .map(|(id, _)| match id {
                0..139 => id + 135,
                139..150 => id + 217,
                _ => id + 379,
            })
            .collect::<Vec<_>>();
            assert_eq!(ids.len(), 153);
            let (mut titles, owners) = party_metadata(&root, disc)?;
            assert_eq!(
                owners,
                crate::session::equipment_owners(&executable, &crate::item::read(&executable)?)?
            );
            assert_eq!(
                serde_json::to_value(&titles)?,
                serde_json::to_value(crate::menu::titles(&executable)?)?
            );
            assert_eq!(
                characters
                    .iter()
                    .map(|&character| costumes::required(
                        character,
                        &titles[usize::from(character - 1)]
                    )
                    .len())
                    .sum::<usize>(),
                36
            );
            // Exercise every shipped selection, including non-title story slots
            // and Lloyd's direct alias. Kratos's absent fifth body stays absent.
            for &character in &characters {
                for costume in [
                    Costume::Standard,
                    Costume::Variant1,
                    Costume::Variant2,
                    Costume::Story,
                    Costume::Variant4,
                ] {
                    if character == 9 && costume == Costume::Variant4 {
                        continue;
                    }
                    let list = &mut titles[usize::from(character - 1)];
                    let mut title = list[0].clone();
                    title.costume = Some(costume);
                    list.push(title);
                }
            }
            let weapons = weapons(&root, disc, &sources, &ids)?;
            let party = party(
                &root,
                disc,
                &sources,
                &characters,
                &titles,
                &weapons,
                &owners,
            )?;
            let pow_weapons = pow_weapons(&root, disc, &sources, &characters)?;
            let pow = &pow_weapons[&PowWeapon::Devastation];
            let pow_devastation = weapons
                .iter()
                .filter(|(item, _)| weapon_owners(&owners, **item) & (1 << 6) != 0)
                .map(|(&item, weapon)| Ok((item, pow_blade::preserve_secondary(weapon, pow)?)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            let (toon_ramp, shadow_texture) = super::super::textures(&root, disc, &sources)?;
            let visuals = VisualAssets {
                arenas: BTreeMap::new(),
                enemies: BTreeMap::new(),
                effect_models: Vec::new(),
                party,
                weapons,
                pow_weapons,
                pow_devastation,
                toon_ramp,
                shadow_texture,
            };
            let pow = &visuals.pow_weapons[&PowWeapon::Devastation];
            visuals.validate()?;
            for scene in visuals.scenes() {
                let prepared = scene
                    .secondary_motion
                    .prepare(&scene.bone_names)
                    .with_context(|| format!("cloth policy for {}", scene.mesh))?;
                assert_eq!(prepared.len(), scene.secondary_motion.chains.len());
            }
            for profile in ["shi00", "zel00", "pre00", "reg00", "kra00"] {
                assert!(
                    visuals.scenes().any(|scene| {
                        scene.secondary_motion.model.contains(profile)
                            && !scene.secondary_motion.is_empty()
                    }),
                    "missing original cloth policy fixture {profile}"
                );
            }
            assert_eq!(
                visuals
                    .party
                    .values()
                    .map(|party| party.bindings.len())
                    .sum::<usize>(),
                44
            );
            assert_eq!(
                visuals
                    .party
                    .values()
                    .map(|party| party.concrete().count())
                    .sum::<usize>(),
                43
            );
            assert_eq!(
                visuals
                    .party
                    .values()
                    .flat_map(PartyVisuals::concrete)
                    .map(|(_, model)| model.weapon_motions.len())
                    .sum::<usize>(),
                75
            );
            assert!(matches!(
                visuals.party[&1].bindings[&Costume::Story],
                PartyVariant::Alias(Costume::Standard)
            ));
            assert_eq!(visuals.pow_weapons.len(), 3);
            assert!(!visuals.pow_devastation.is_empty());
            let directory = Directory::open(&root, disc, &sources.usual)?;
            let weapon_directory = Directory::open(&root, disc, sources.archive(Archive::Weapon))?;
            let magic_directory = Directory::open(&root, disc, sources.archive(Archive::Magic))?;
            let check = |name: &str, visual: &Visual| {
                super::super::tests::assert_bound(&directory, name, visual)
            };
            for (&item, weapon) in &visuals.weapons {
                super::super::tests::assert_bound(
                    &weapon_directory,
                    &format!("weapon-{item}"),
                    &Visual::Weapon(weapon.clone()),
                )?;
            }
            for (&character, party) in &visuals.party {
                for &costume in party.bindings.keys() {
                    let mut model = party.resolve(costume)?.1.clone();
                    for (item, motion) in std::mem::take(&mut model.weapon_motions) {
                        super::super::tests::assert_bound(
                            &weapon_directory,
                            &format!("weapon-{item}-costume-{}", costume as u8),
                            &Visual::WeaponMotions(motion),
                        )?;
                    }
                    // Aliases share complete body/CAB data but use their canonical mesh namespace.
                    if let PartyVariant::Model(_) = party.bindings[&costume] {
                        check(
                            &format!("party-{character}-costume-{}", costume as u8),
                            &Visual::Party(model),
                        )?;
                    }
                }
            }
            for (&kind, weapon) in &visuals.pow_weapons {
                super::super::tests::assert_bound(
                    &magic_directory,
                    &format!("pow-weapon-{}", kind.native()),
                    &Visual::Weapon(weapon.clone()),
                )?;
            }
            for (&item, weapon) in &visuals.pow_devastation {
                assert_eq!(
                    serde_json::to_value((
                        &weapon.slots[&0],
                        &weapon.rigs[&0],
                        &weapon.trails[&0]
                    ))?,
                    serde_json::to_value((&pow.slots[&0], &pow.rigs[&0], &pow.trails[&0]))?
                );
                assert_eq!(
                    serde_json::to_value((
                        &weapon.slots[&1],
                        &weapon.rigs[&1],
                        weapon.trails.get(&1)
                    ))?,
                    serde_json::to_value((
                        &visuals.weapons[&item].slots[&1],
                        &visuals.weapons[&item].rigs[&1],
                        visuals.weapons[&item].trails.get(&1)
                    ))?
                );
            }
            check("toon-ramp", &Visual::Texture(visuals.toon_ramp))?;
            check("shadow", &Visual::Texture(visuals.shadow_texture))?;
        }
        Ok(())
    }
}
