//! Initial publication names come from shared numeric identities, not parallel
//! cooker-only JSON catalogues. Localized display text remains original data.
use anyhow::Result;
use resonance_content::{library::Mappings, overworld::PartyCharacter};
use std::collections::BTreeMap;
use symphonia_constants::{KnownId, TitleId};

pub(crate) fn names<T: KnownId>() -> BTreeMap<u32, String> {
    T::ALL
        .iter()
        .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.into())))
        .collect()
}

pub(crate) fn mappings<T: KnownId>() -> Result<Mappings> {
    Ok(Mappings::new(names::<T>())?)
}

pub(crate) fn titles() -> Result<BTreeMap<PartyCharacter, Mappings>> {
    let mut characters = BTreeMap::<PartyCharacter, BTreeMap<u32, String>>::new();
    for &id in &TitleId::ALL {
        let character = PartyCharacter::try_from((id.raw() >> 8) as u8 + 1)?;
        characters.entry(character).or_default().insert(
            u32::from(id.raw() & 255),
            id.resource_name().unwrap().into(),
        );
    }
    characters
        .into_iter()
        .map(|(character, names)| Ok((character, Mappings::new(names)?)))
        .collect()
}
