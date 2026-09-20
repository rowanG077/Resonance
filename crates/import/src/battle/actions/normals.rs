//! Normal-attack selectors and bundles, including unused authored groups.
use super::*;
use crate::battle::embedded::{self, Layout};
use serde::{Deserialize, Serialize};

const VARIANTS: usize = 7;

#[derive(Serialize, Deserialize)]
pub(crate) struct Group {
    /// Authored group indices start at one.
    index: u8,
    selectors: Vec<Selector>,
    bundles: Vec<Bundle>,
}

#[derive(Serialize, Deserialize)]
struct Selector {
    bundle: u8,
    allowed_directions: u8,
    fallback: Option<u8>,
}

#[derive(Serialize, Deserialize)]
struct Bundle {
    combo_first: u16,
    combo_second: Option<u16>,
    buffer_until: u8,
    recovery: Recovery,
    effect: Option<u16>,
    reach: u16,
    airborne_reach: u16,
    action: Action,
}

pub(crate) fn embedded(path: &Path) -> Result<Option<Vec<Group>>> {
    let Some((_, layout)) = Layout::identify(path) else {
        return Ok(None);
    };
    let rel = Rel::read(path)?;
    (1..=layout.normal_groups)
        .map(|index| group(&rel, layout.normal_actions, index))
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some(groups) = embedded(file)? else {
        return Ok(None);
    };
    let (_, layout) = Layout::identify(file).unwrap();
    embedded::write(file, output, "battle-normal-actions", &groups, serde_json::json!({
        "section": DATA, "offset": layout.normal_actions, "stride": 12, "count": layout.normal_groups,
    })).map(Some)
}

pub(super) fn bind(data: &Path, characters: &[u8]) -> Result<Vec<PartyActions>> {
    let groups: Vec<Group> =
        crate::embedded::read(data, "battle-normal-actions", "US_r_Top2Btl.rel")?;
    select(&groups, characters)
}

fn select(groups: &[Group], characters: &[u8]) -> Result<Vec<PartyActions>> {
    let mut indexed = BTreeMap::new();
    for group in groups {
        ensure!(
            group.index != 0 && indexed.insert(group.index, group).is_none(),
            "invalid or duplicate normal group identity {}",
            group.index
        );
        ensure!(
            group.selectors.len() == VARIANTS
                && group.selectors.iter().all(|selector| {
                    usize::from(selector.bundle) < group.bundles.len()
                        && selector
                            .fallback
                            .is_none_or(|index| usize::from(index) < group.selectors.len())
                }),
            "invalid normal selectors in group {}",
            group.index
        );
    }
    characters
        .iter()
        .map(|character| {
            indexed
                .get(character)
                .map(|group| project(group))
                .with_context(|| format!("missing cooked normal group {character}; rerun cook-all"))
        })
        .collect()
}

#[cfg(test)]
pub(in crate::battle) fn party(rel: &Rel, character: u8) -> Result<PartyActions> {
    Ok(project(&group(
        rel,
        Layout::RETAIL.normal_actions,
        character,
    )?))
}

fn project(group: &Group) -> PartyActions {
    let normal = group
        .selectors
        .iter()
        .enumerate()
        .map(|(selection, selector)| {
            let bundle = &group.bundles[usize::from(selector.bundle)];
            NormalAction {
                selection: selection as u8,
                combo: ComboWindow {
                    first: bundle.combo_first,
                    second: bundle.combo_second,
                    buffer_until: bundle.buffer_until,
                    allowed_directions: selector.allowed_directions,
                    fallback: selector.fallback,
                },
                recovery: bundle.recovery,
                effect: bundle.effect,
                reach: bundle.reach,
                airborne_reach: bundle.airborne_reach,
                action: bundle.action.clone(),
            }
        })
        .collect();
    PartyActions {
        character: group.index,
        normal,
    }
}

fn group(rel: &Rel, root: usize, index: u8) -> Result<Group> {
    let table = root + usize::from(index.checked_sub(1).context("zero normal group")?) * 12;
    let bundles = rel.pointer(DATA, table)?;
    let rules = rel.at(rel.pointer(DATA, table + 4)?)?;
    let selectors = rel.at(rel.pointer(DATA, table + 8)?)?;
    let selectors = (0..VARIANTS)
        .map(|index| {
            let row = selectors
                .get(index * 4..index * 4 + 4)
                .context("truncated normal selector")?;
            ensure!(
                usize::from(row[0]) < VARIANTS && row[3] == 0,
                "invalid normal selector"
            );
            Ok(Selector {
                bundle: row[0],
                allowed_directions: row[1],
                fallback: (row[2] != 255).then_some(row[2]),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let bundles = (0..VARIANTS)
        .map(|index| {
            let at =
                |slot: usize| rel.at(rel.pointer(bundles.0, bundles.1 + index * 16 + slot * 4)?);
            let descriptor = at(0)?.get(..24).context("truncated normal descriptor")?;
            ensure!(
                descriptor[10..12] == [0, 0],
                "nonzero normal descriptor padding"
            );
            let (commands, loop_commands) = commands(at(3)?)?;
            let animation = rel.pointer(bundles.0, bundles.1 + index * 16 + 8)?;
            Ok(Bundle {
                combo_first: half(descriptor, 4)?,
                combo_second: nonzero(half(descriptor, 6)?),
                buffer_until: descriptor[8],
                recovery: Recovery {
                    duration: half(descriptor, 2)?,
                    animation: (descriptor[9] != 0).then_some(descriptor[9]),
                    rate: float(descriptor, 12)?,
                },
                effect: nonzero(
                    word(descriptor, 16)?
                        .try_into()
                        .context("invalid normal effect")?,
                ),
                reach: half(descriptor, 20)?,
                airborne_reach: half(descriptor, 22)?,
                action: Action {
                    duration: half(descriptor, 0)?,
                    tp: 0,
                    animations: animations_at(rel.at((animation.0, 0))?, animation.1)?,
                    commands,
                    loop_commands,
                    hits: hits(at(1)?, rules)?,
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Group {
        index,
        selectors,
        bundles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_selection_checks_identities_and_authored_indices() -> Result<()> {
        let bundle = |duration| Bundle {
            combo_first: 4,
            combo_second: None,
            buffer_until: 8,
            recovery: Recovery {
                duration: 3,
                animation: None,
                rate: 1.,
            },
            effect: None,
            reach: 10,
            airborne_reach: 20,
            action: Action {
                duration,
                tp: 0,
                animations: Default::default(),
                commands: vec![],
                loop_commands: false,
                hits: vec![],
            },
        };
        let mut groups = vec![Group {
            index: 2,
            selectors: (0..VARIANTS)
                .map(|index| Selector {
                    bundle: (index % 2) as u8,
                    allowed_directions: 15,
                    fallback: Some(0),
                })
                .collect(),
            bundles: vec![bundle(12), bundle(24)],
        }];
        let party = select(&groups, &[2])?;
        assert_eq!(party[0].character, 2);
        assert_eq!(
            party[0]
                .normal
                .iter()
                .map(|normal| normal.action.duration)
                .collect::<Vec<_>>(),
            [12, 24, 12, 24, 12, 24, 12]
        );
        assert!(select(&groups, &[1]).is_err());
        groups[0].selectors[0].bundle = 2;
        assert!(select(&groups, &[2]).is_err());
        groups[0].selectors[0].bundle = 0;
        groups[0].selectors[0].fallback = Some(VARIANTS as u8);
        assert!(select(&groups, &[2]).is_err());
        groups[0].selectors[0].fallback = None;
        groups[0].index = 0;
        assert!(select(&groups, &[0]).is_err());
        groups[0].index = 2;
        groups.push(Group {
            index: 2,
            selectors: vec![],
            bundles: vec![],
        });
        assert!(select(&groups, &[2]).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires original battle modules and cook-all publications; no asset conversion"]
    fn published_normal_groups_match_both_discs_and_preserve_every_bundle() -> Result<()> {
        let value = |groups: &[Group]| serde_json::to_value(groups).unwrap();
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in ["disc1", "disc2"] {
            let files = local.join("extracted").join(disc).join("files");
            let compare = |module: &str| -> Result<Vec<Group>> {
                let original = embedded(&files.join(module))?.unwrap();
                let cooked: Vec<Group> = crate::embedded::read(
                    &local.join("all-assets/data"),
                    "battle-normal-actions",
                    module,
                )?;
                assert_eq!(value(&cooked), value(&original), "{disc}/{module}");
                let identities: Vec<_> = original.iter().map(|group| group.index).collect();
                assert_eq!(
                    serde_json::to_value(select(&cooked, &identities)?)?,
                    serde_json::to_value(original.iter().map(project).collect::<Vec<_>>())?,
                    "{disc}/{module} selected projection",
                );
                Ok(original)
            };
            let active = compare("US_r_Top2Btl.rel")?;
            assert_eq!(active.len(), 9);
            assert_eq!(value(&active), value(&compare("r_Top2Btl.rel")?));
            assert_eq!(value(&active), value(&compare("Top2BtlD.rel")?));
            let mut larger = None;
            for name in [
                "US_Top2Btl.rel",
                "US_m_Top2Btl.rel",
                "Top2Btl.rel",
                "m_Top2Btl.rel",
            ] {
                let groups = compare(name)?;
                assert_eq!(groups.len(), 11);
                assert_eq!(value(&groups[..9]), value(&active));
                if let Some(previous) = &larger {
                    assert_eq!(&value(&groups), previous);
                }
                larger = Some(value(&groups));
                for extra in &groups[9..] {
                    assert_eq!(extra.selectors.len(), VARIANTS);
                    assert_eq!(extra.bundles.len(), VARIANTS);
                    assert!(extra.selectors.iter().all(|selector| selector.bundle != 6));
                    assert_eq!(
                        serde_json::to_value(&extra.bundles[5])?,
                        serde_json::to_value(&extra.bundles[6])?
                    );
                    assert!(
                        active
                            .iter()
                            .all(|group| serde_json::to_value(&group.bundles).unwrap()
                                != serde_json::to_value(&extra.bundles).unwrap())
                    );
                }
                let rel = Rel::read(&files.join(name))?;
                assert_eq!(rel.pointer(DATA, 0x4780)?.0, 1);
            }
        }
        Ok(())
    }
}
