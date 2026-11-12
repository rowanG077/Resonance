//! Runtime IDs for catalogue entries which a mod identifies by name alone.
//!
//! Original entries keep the numeric IDs that scripts and native tables use.
//! An added entry without a `mappings` registration receives the next free ID
//! when the library opens, so independent mods never compete for a number.
//! Saves record these entries by name; see `Identities`.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

const ALLOCATED_FROM: &str = "allocated_from";

/// Catalogues with allocation: document, entry collection, first ID past the
/// namespace, and whether ID zero is a usable entry.
const CATALOGUES: [(&str, &str, u32, u32); 2] = [
    (crate::items::PATH, "items", 4096, 0),
    (crate::arte::PATH, "techniques", 1 << 16, 1),
];

/// Register every unmapped entry after the last explicit ID, in name order.
/// The assembled view then looks like any fully registered catalogue.
pub(crate) fn allocate(document: &str, value: &mut Value) -> Result<()> {
    let Some(&(_, collection, limit, first)) =
        CATALOGUES.iter().find(|(path, ..)| *path == document)
    else {
        return Ok(());
    };
    let root = value
        .as_object_mut()
        .with_context(|| format!("{document} is not an object"))?;
    // The boundary is derived from the enabled layers, never authored.
    root.remove(ALLOCATED_FROM);
    let mut unmapped: Vec<String> = root
        .get(collection)
        .and_then(Value::as_object)
        .map(|entries| entries.keys().cloned().collect())
        .unwrap_or_default();
    let mappings = root
        .entry("mappings")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .with_context(|| format!("{document} mappings are not an object"))?;
    unmapped.retain(|name| {
        !mappings
            .values()
            .any(|mapped| mapped.as_str() == Some(name))
    });
    if unmapped.is_empty() {
        return Ok(());
    }
    unmapped.sort();
    let mut next = first;
    for key in mappings.keys() {
        // Typed catalogue readers report a malformed key with its context.
        if let Ok(id) = key.parse::<u32>() {
            next = next.max(id.saturating_add(1));
        }
    }
    let boundary = next;
    for name in unmapped {
        ensure!(
            next < limit,
            "{document} has no free ID left for {name}; too many added entries"
        );
        mappings.insert(next.to_string(), name.into());
        next += 1;
    }
    root.insert(ALLOCATED_FROM.into(), boundary.into());
    Ok(())
}

/// Names of the allocated entries of a prepared catalogue, by current ID.
pub fn allocated(
    mappings: &crate::library::Mappings,
    from: Option<u32>,
) -> Result<std::collections::BTreeMap<u16, String>> {
    let Some(from) = from else {
        return Ok(Default::default());
    };
    mappings
        .iter()
        .filter(|(id, _)| *id >= from)
        .map(|(id, name)| {
            Ok((
                u16::try_from(id).context("allocated ID exceeds u16")?,
                name.into(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unmapped_entries_follow_explicit_ids_in_name_order() -> Result<()> {
        let mut items = json!({
            "mappings": {"0": "none", "1": "apple-gel"},
            "items": {"none": {}, "zest": {}, "apple-gel": {}, "berry": {}},
            "allocated_from": 99
        });
        allocate(crate::items::PATH, &mut items)?;
        assert_eq!(
            items["mappings"],
            json!({"0": "none", "1": "apple-gel", "2": "berry", "3": "zest"})
        );
        assert_eq!(items[ALLOCATED_FROM], 2);

        let mut complete = json!({"mappings": {"0": "none"}, "items": {"none": {}}});
        allocate(crate::items::PATH, &mut complete)?;
        assert!(complete.get(ALLOCATED_FROM).is_none());

        let mut techniques = json!({"techniques": {"ghost-wolf": {}}});
        allocate(crate::arte::PATH, &mut techniques)?;
        assert_eq!(techniques["mappings"], json!({"1": "ghost-wolf"}));

        let mut other = json!({"items": {"a": {}}});
        allocate("game/titles.json", &mut other)?;
        assert!(other.get("mappings").is_none());
        Ok(())
    }

    #[test]
    fn a_full_namespace_names_the_entry_without_an_id() {
        let mut items = json!({"mappings": {"4095": "last"}, "items": {"last": {}, "extra": {}}});
        let error = allocate(crate::items::PATH, &mut items).unwrap_err();
        assert!(error.to_string().contains("extra"), "{error}");
    }
}
