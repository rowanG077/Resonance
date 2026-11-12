//! The example mods under `mods/` against a cooked library: each one loads
//! through the ordinary mod layer and binds what its README promises.
use anyhow::{Context, Result};
use resonance_content::{
    diagnostics::Diagnostics,
    items::BattleUse,
    library::Library,
    menu_data::{MenuData, ShopTrade},
    prepared::Files,
    session::SessionData,
};
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Loaded {
    session: SessionData,
    menus: MenuData,
}

impl Loaded {
    /// Open the cooked library with the named example mods, in order.
    fn open(mods: &[&str]) -> Result<Self> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|| repository().join("local/all-assets"));
        let mods: Vec<_> = mods
            .iter()
            .map(|name| repository().join("mods").join(name))
            .collect();
        let library = Library::open_paths(&root, &mods)?;
        let report = resonance_content::validation::validate(&library);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let files: Files = resonance_content::preparation::capture(
            &library,
            &[],
            || false,
            Diagnostics::new(true),
        )?;
        Ok(Self {
            session: SessionData::load(&files)?,
            menus: MenuData::load(&files)?,
        })
    }

    fn item(&self, name: &str) -> Result<u16> {
        self.session
            .identities
            .items
            .iter()
            .find_map(|(&id, allocated)| (allocated == name).then_some(id))
            .with_context(|| format!("mod item {name} received no ID"))
    }
}

/// Halo is the Iselia shop, world-map shop 1.
const HALO: usize = 1;
/// Monster List entry of Fake.
const FAKE: u8 = 73;

#[test]
#[ignore = "requires cooked original assets; set RESONANCE_TEST_ASSETS or cook local/all-assets"]
fn all_multiply_is_sold_in_iselia_dropped_by_fakes_and_usable_in_battle() -> Result<()> {
    let loaded = Loaded::open(&["all-multiply"])?;
    let id = loaded.item("all-multiply")?;
    let item = &loaded.menus.items[usize::from(id)];
    assert!(item.battle_usable);
    assert_eq!(item.battle_use, Some(BattleUse::AllMultiply));
    assert_eq!(item.shop_price(ShopTrade::Buy, false), 500);
    assert_eq!(loaded.menus.world_map.shops[HALO].items.last(), Some(&id));
    let drops = &loaded.menus.battle_rewards.monsters[&FAKE];
    assert_eq!(drops.len(), 1);
    assert_eq!(
        (drops[0].item, drops[0].chance, drops[0].count),
        (id, 15, 1)
    );
    Ok(())
}

/// Stock of a shop by its library name.
fn shop(loaded: &Loaded, name: &str) -> Result<Vec<u16>> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repository().join("local/all-assets"));
    let library = Library::open_paths(&root, &[])?;
    let document: serde_json::Value = library.snapshot().json(
        resonance_content::menu_data::WORLD_MAP_PATH,
        resonance_content::preparation::JSON_LIMIT,
    )?;
    let shops: resonance_content::library::Mappings =
        serde_json::from_value(document["mappings"].clone())?;
    Ok(loaded.menus.world_map.shops[shops.id(name)? as usize]
        .items
        .clone())
}

#[test]
#[ignore = "requires cooked original assets; set RESONANCE_TEST_ASSETS or cook local/all-assets"]
fn unison_force_is_sold_in_palmacosta_and_fills_the_gauge() -> Result<()> {
    let loaded = Loaded::open(&["unison-force"])?;
    let id = loaded.item("unison-force")?;
    let item = &loaded.menus.items[usize::from(id)];
    assert!(item.battle_usable);
    assert_eq!(item.battle_use, Some(BattleUse::UnisonGauge));
    assert_eq!(item.shop_price(ShopTrade::Buy, false), 1200);
    assert_eq!(shop(&loaded, "marble-s")?.last(), Some(&id));
    Ok(())
}

#[test]
#[ignore = "requires cooked original assets; set RESONANCE_TEST_ASSETS or cook local/all-assets"]
fn unlock_limit_is_sold_in_meltokio_and_grants_over_limit() -> Result<()> {
    let loaded = Loaded::open(&["unlock-limit"])?;
    let id = loaded.item("unlock-limit")?;
    let item = &loaded.menus.items[usize::from(id)];
    assert!(item.battle_usable);
    assert_eq!(item.battle_use, Some(BattleUse::OverLimit));
    assert_eq!(item.shop_price(ShopTrade::Buy, false), 3000);
    assert_eq!(shop(&loaded, "belteon")?.last(), Some(&id));
    Ok(())
}

/// Independent item mods receive distinct IDs and keep their own shop lines.
#[test]
#[ignore = "requires cooked original assets; set RESONANCE_TEST_ASSETS or cook local/all-assets"]
fn the_item_mods_load_together_without_sharing_an_id() -> Result<()> {
    let loaded = Loaded::open(&["all-multiply", "unison-force", "unlock-limit"])?;
    let ids =
        ["all-multiply", "unison-force", "unlock-limit"].map(|name| loaded.item(name).unwrap());
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    assert_eq!(shop(&loaded, "halo")?.last(), Some(&ids[0]));
    assert_eq!(shop(&loaded, "marble-s")?.last(), Some(&ids[1]));
    assert_eq!(shop(&loaded, "belteon")?.last(), Some(&ids[2]));
    Ok(())
}

#[test]
#[ignore = "requires cooked original assets; set RESONANCE_TEST_ASSETS or cook local/all-assets"]
fn momentum_ring_is_an_accessory_sold_in_luin_with_combo_damage() -> Result<()> {
    let loaded = Loaded::open(&["momentum-ring"])?;
    let id = loaded.item("momentum-ring")?;
    let item = &loaded.menus.items[usize::from(id)];
    assert_eq!(
        (
            item.properties.combo_damage.per_hit,
            item.properties.combo_damage.limit
        ),
        (1, 20)
    );
    assert_eq!(item.shop_price(ShopTrade::Buy, false), 6000);
    assert_eq!(shop(&loaded, "fighting-spirit")?.last(), Some(&id));
    // Every character can wear it in either accessory slot, and only there.
    let definition = &loaded.session.items[usize::from(id)];
    for character in 0..9 {
        assert!(definition.fits_slot(character, 3) && definition.fits_slot(character, 4));
        assert!(!definition.fits_slot(character, 0));
    }
    assert!(
        loaded
            .menus
            .status
            .equipment_effects
            .contains_key("momentum-combo-damage")
    );
    Ok(())
}
