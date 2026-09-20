use super::*;
use crate::{
    battle::all::{Archive, NativeResources, PartySettings, Sources},
    cooked::Source,
};
use std::collections::BTreeMap;

pub(super) struct Package {
    pub resources: NativeResources,
    pub actions: Bundle,
}

pub(in crate::battle) struct Inputs {
    packages: BTreeMap<u16, Package>,
    party: Vec<PartySettings>,
    pub(super) parameters: parameters::Parameters,
}

fn natives() -> impl Iterator<Item = u16> {
    PowWeapon::ALL
        .into_iter()
        .map(PowWeapon::native)
        .chain(Thrust::ALL.into_iter().map(Thrust::native))
        .chain(Strike::ALL.into_iter().map(Strike::native))
        .chain(CombinedPair::ALL.into_iter().map(CombinedPair::native))
}

impl Inputs {
    pub fn bind(output: &Path, disc: u8, sources: &Sources) -> Result<Self> {
        let source = Source::open(output, disc, sources.archive(Archive::Magic))?;
        Ok(Self {
            packages: natives()
                .map(|native| {
                    let package = native - 200;
                    let (_, bytes) =
                        source.resolve(&format!("battle/all/magic-{package}/resources.json"))?;
                    let resources: NativeResources = serde_json::from_slice(&bytes)?;
                    resources.validate()?;
                    ensure!(
                        resources.actions != 0,
                        "missing Unison action bundle {native}"
                    );
                    Ok((
                        native,
                        Package {
                            resources,
                            actions: Bundle::bind(
                                &source,
                                &format!("battle/magic-{package}/member-100/actions.json"),
                            )?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            party: Source::open(output, disc, "US_r_Top2Btl.rel")?
                .embedded("battle-party-settings", "US_r_Top2Btl.rel")?,
            parameters: Source::open(output, disc, "US_r_Top2Btl.rel")?
                .embedded("battle-unison-parameters", "US_r_Top2Btl.rel")?,
        })
    }

    pub(super) fn package(&self, native: u16) -> Result<&Package> {
        self.packages
            .get(&native)
            .context("missing cooked Unison package")
    }

    pub(super) fn actor(&self, character: u8) -> Result<&crate::battle::all::ActorSettings> {
        self.party
            .iter()
            .find(|row| row.character == character)
            .map(|row| &row.settings)
            .context("missing cooked Unison actor settings")
    }

    /// Source inventory uses the same physical decoders without requiring a prior cook.
    pub(in crate::battle) fn read_source(extracted: &Path) -> Result<Self> {
        use crate::battle::{
            effect_program::MagicArchive,
            embedded::{Layout, SETTINGS_BYTES},
        };
        let archive = MagicArchive::read(extracted)?;
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
        Ok(Self {
            packages: natives()
                .map(|native| {
                    let bytes = archive.package(native - 200)?;
                    let resources = NativeResources::read(bytes)?;
                    let range = resources
                        .member(256)?
                        .context("missing Unison action bundle")?;
                    Ok((
                        native,
                        Package {
                            resources,
                            actions: Bundle::decode(&bytes[range])?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            party: (1..=9)
                .map(|character| {
                    Ok(PartySettings {
                        character,
                        settings: crate::battle::all::ActorSettings::read(rel.at((
                            5,
                            Layout::RETAIL.party_settings
                                + usize::from(character - 1) * SETTINGS_BYTES,
                        ))?)?,
                    })
                })
                .collect::<Result<_>>()?,
            parameters: parameters::Parameters::read(&rel)?,
        })
    }
}

#[test]
#[ignore = "requires both original discs and cooked Unison publications"]
fn original_cooked_unison_inputs_match_shared_source_decoders() -> Result<()> {
    use crate::battle::{embedded::Layout, unison_opener, unison_tables};
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let output = local.join("all-assets");
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let original = Inputs::read_source(&extracted)?;
        let cooked = Inputs::bind(&output, disc, &Sources::cooked(&output, disc)?)?;
        for native in natives() {
            assert_eq!(
                serde_json::to_vec(&original.package(native)?.resources)?,
                serde_json::to_vec(&cooked.package(native)?.resources)?,
                "disc {disc}, native {native} resource bindings",
            );
        }
        let module = Source::open(&output, disc, "US_r_Top2Btl.rel")?;
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
        let expected = super::cook(
            &original,
            &unison_tables::read(&rel, &Layout::RETAIL)?,
            &unison_opener::read(&rel, &Layout::RETAIL)?,
            &crate::arte::read(&fs::read(extracted.join("sys/main.dol"))?)?,
        )?;
        let actual = super::cook(
            &cooked,
            &module.embedded("battle-unison-tables", "US_r_Top2Btl.rel")?,
            &module.embedded("battle-unison-opener", "US_r_Top2Btl.rel")?,
            &crate::arte::cooked(&output.join("data"))?,
        )?;
        assert_eq!(
            serde_json::to_vec(&actual)?,
            serde_json::to_vec(&expected)?,
            "disc {disc} Unison"
        );
    }
    Ok(())
}
