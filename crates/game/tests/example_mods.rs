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
