//! Every recovered inventory runs through the same native service and basket UI.
#[path = "common/action.rs"]
mod action;
mod common;
use action::Action::{self, *};
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::{EventRuntime, GameWorld, ResourceLibrary, party::Party};
use resonance_game::{
    field::shop::{Choice, Focus, Shop},
    menu::Resources,
};
use std::sync::Arc;

fn resources() -> Arc<Resources> {
    let data: MenuData = common::cooked("game/menu-data.json");
    let mut session: SessionData = common::cooked("game/session-data.json");
    session.ex_skills = Some(Arc::new(data.ex_skills.clone()));
    Arc::new(Resources {
        session: Arc::new(session),
        data: Arc::new(data),
    })
}

fn request(resources: &Arc<Resources>, selector: u16, party: Party) -> EventRuntime {
    // The real merchant discards OpenMenu's result before continuing its event.
    let main = [
        0x0200, selector, 0, 0x3000, 0x4000, 0x2067, 0x3000, 0x0200, 511, 0, 0x3000, 0x4000,
        0x2068, 0x20ff,
    ];
    let words: Vec<u16> = [10, 0, 0, 1, 0, 2, 0, 42, 0, main.len() as u16]
        .into_iter()
        .chain(main)
        .chain([0x20ff])
        .collect();
    let program = symphonia_script::Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut world = GameWorld::default();
    world.party = Some(party);
    let mut memory = symphonia_script_vm::Memory::default();
    for address in [0x20, 0x24, 0x28] {
        memory
            .write(address, symphonia_script::Width::S32, 31)
            .unwrap();
    }
    let events = EventRuntime::with_state(
        Arc::new(program),
        Arc::new(ResourceLibrary {
            session_data: Some(resources.session.clone()),
            menu_data: Some(resources.data.clone()),
            ..Default::default()
        }),
        world,
        memory,
    )
    .unwrap();
    for address in [0x24, 0x28] {
        assert_eq!(
            events
                .memory()
                .read(address, symphonia_script::Width::S32)
                .unwrap(),
            31
        );
    }
    events
}

fn assert_resumed(events: &mut EventRuntime) {
    assert!(!events.world.event_flags.contains(&511));
    events.step().unwrap();
    assert!(events.world.event_flags.contains(&511));
    for address in [0x20, 0x24, 0x28] {
        assert_eq!(
            events
                .memory()
                .read(address, symphonia_script::Width::S32)
                .unwrap(),
            0
        );
    }
}

fn open(resources: &Arc<Resources>, id: u8, party: Party) -> (EventRuntime, Shop) {
    let mut events = request(resources, u16::from(id), party);
    let request = events.world.menu_request.take().unwrap();
    assert_eq!(request.target, resonance_events::menu::Target::Shop(id));
    assert!(request.operation.is_pending());
    assert!(!events.world.event_flags.contains(&511));
    let mut shop = Shop::open(
        id,
        resources.clone(),
        events.world.party.as_mut().unwrap(),
        request.operation,
    )
    .unwrap();
    for _ in 0..10 {
        shop.step(Default::default(), events.world.party.as_mut().unwrap())
            .unwrap();
    }
    (events, shop)
}

fn press(shop: &mut Shop, party: &mut Party, action: Action) {
    shop.step(action.into(), party).unwrap();
    for _ in 0..3 {
        shop.step(Default::default(), party).unwrap();
    }
}

#[test]
#[ignore = "requires locally cooked shops and session definitions; no devices"]
fn all_shop_stock_can_be_bought_and_sold_without_losing_native_continuation() {
    let resources = resources();
    assert_eq!(resources.data.world_map.shops.len(), 52);
    let mut stock_count = 0;
    for (id, stock) in resources.data.world_map.shops.iter().enumerate() {
        let mut party = Party::new(&resources.session, Default::default()).unwrap();
        party.gald = 99_999_999;
        let (mut events, mut shop) = open(&resources, id as u8, party);
        let party = events.world.party.as_mut().unwrap();
        assert!(party.travel.visited_shops.contains(&(id as u8)));
        press(&mut shop, party, Accept);
        assert_eq!(
            shop.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            stock.items
        );
        let sale_total: u32 = stock
            .items
            .iter()
            .map(|&id| resources.data.items[usize::from(id)].price)
            .sum();
        for row in 0..stock.items.len() {
            assert_eq!(shop.row, row);
            press(&mut shop, party, Right);
            if row + 1 < stock.items.len() {
                press(&mut shop, party, Down);
            }
        }
        assert_eq!(shop.total(party), sale_total * 2);
        assert!(
            party.items.is_empty(),
            "basket must not transfer before confirmation"
        );
        press(&mut shop, party, Accept);
        assert_eq!(shop.focus, Focus::Confirm { yes: true });
        press(&mut shop, party, Cancel);
        assert_eq!(
            shop.total(party),
            sale_total * 2,
            "cancel confirmation retains basket"
        );
        press(&mut shop, party, Accept);
        press(&mut shop, party, Accept);
        assert_eq!(shop.focus, Focus::Root);
        assert_eq!(party.gald, 99_999_999 - sale_total * 2);
        assert_eq!(party.spent_gald, sale_total * 2);
        assert!(
            stock
                .items
                .iter()
                .all(|id| party.items.get(id) == Some(&1) && party.found_items.contains(id))
        );
        stock_count += stock.items.len();

        press(&mut shop, party, Right);
        assert_eq!(shop.choice, Choice::Sell);
        press(&mut shop, party, Accept);
        for category in 0..7 {
            assert_eq!(shop.category, category);
            assert_eq!(shop.focus, Focus::Categories);
            if !shop.rows.is_empty() {
                press(&mut shop, party, Accept);
                for row in 0..shop.rows.len() {
                    press(&mut shop, party, Alternate);
                    if row + 1 < shop.rows.len() {
                        press(&mut shop, party, Down);
                    }
                }
                press(&mut shop, party, Accept);
                press(&mut shop, party, Accept);
            }
            if category != 6 {
                press(&mut shop, party, Right);
            }
        }
        assert!(party.items.is_empty(), "shop {id} left unsold stock");
        assert_eq!(party.gald, 99_999_999 - sale_total);
        press(&mut shop, party, Cancel);
        press(&mut shop, party, Cancel);
        for _ in 0..10 {
            shop.step(Default::default(), party).unwrap();
        }
        assert!(shop.closed);
        assert_resumed(&mut events);
    }
    assert_eq!(stock_count, 608);
}

#[test]
#[ignore = "requires locally cooked shops and session definitions; no devices"]
fn cancelling_shop_or_main_menu_returns_zero_and_clears_script_status() {
    let resources = resources();
    let party = Party::new(&resources.session, Default::default()).unwrap();
    let (mut events, mut shop) = open(&resources, 1, party.clone());
    press(&mut shop, events.world.party.as_mut().unwrap(), Cancel);
    for _ in 0..10 {
        shop.step(Default::default(), events.world.party.as_mut().unwrap())
            .unwrap();
    }
    assert!(shop.closed);
    assert_resumed(&mut events);

    let mut events = request(&resources, 9995, party);
    let request = events.world.menu_request.take().unwrap();
    assert_eq!(request.target, resonance_events::menu::Target::Main);
    events.step().unwrap();
    assert!(!events.world.event_flags.contains(&511));
    request.operation.complete(Some(0)).unwrap();
    assert_resumed(&mut events);
}

#[test]
#[ignore = "requires locally cooked shops and session definitions; no devices"]
fn basket_limits_personal_discount_equipment_return_and_cancel_are_transactional() {
    let resources = resources();
    let mut party = Party::new(&resources.session, Default::default()).unwrap();
    party.gald = 199;
    let (mut events, mut shop) = open(&resources, 1, party);
    let party = events.world.party.as_mut().unwrap();
    press(&mut shop, party, Accept);
    press(&mut shop, party, PageDown);
    assert_eq!((shop.row, shop.first), (7, 7));
    press(&mut shop, party, PageDown);
    assert_eq!((shop.row, shop.first), (13, 7));
    press(&mut shop, party, PageUp);
    assert_eq!((shop.row, shop.first), (6, 0));
    press(&mut shop, party, PageUp);
    assert_eq!((shop.row, shop.first), (0, 0));
    let id = shop.rows[0].id;
    let base = resources.data.items[usize::from(id)].price;
    let price = base * 2;
    party.gald = price - 1;
    press(&mut shop, party, Accept);
    assert_eq!(shop.focus, Focus::Items);
    assert_eq!(shop.rows[0].quantity, 0);
    party.gald = price * 3;
    press(&mut shop, party, Alternate);
    assert_eq!(shop.rows[0].quantity, 3);
    press(&mut shop, party, Right);
    assert_eq!(shop.rows[0].quantity, 3, "basket must respect total funds");
    press(&mut shop, party, Cancel);
    assert!(party.items.is_empty());
    press(&mut shop, party, Accept);
    assert_eq!(
        shop.total(party),
        0,
        "re-entering Buy discards the cancelled basket"
    );
    party.gald = 99_999_999;
    let cap = resources.session.items[usize::from(id)].stack_limit;
    party
        .change_item(&resources.session, id, cap as i8 - 1)
        .unwrap();
    press(&mut shop, party, Alternate);
    assert_eq!(
        shop.rows[0].quantity, 1,
        "owned items consume basket capacity"
    );
    press(&mut shop, party, Accept);
    press(&mut shop, party, Down);
    press(&mut shop, party, Accept);
    assert_eq!(
        party.items[&id],
        cap - 1,
        "No leaves the inventory unchanged"
    );
    assert_eq!(shop.focus, Focus::Items);
    party.formation.push(8);
    party.members[7].ex_skills[0] = 50;
    assert_eq!(shop.unit_price(id, party), base * 180 / 100);
    party.formation.pop();
    assert_eq!(
        shop.unit_price(id, party),
        price,
        "Personal requires Regal in formation"
    );
    press(&mut shop, party, Cancel);
    press(&mut shop, party, Right);
    press(&mut shop, party, Accept);
    press(&mut shop, party, Accept);
    party.formation.push(8);
    party.gald = 99_999_998;
    assert_eq!(shop.unit_price(id, party), base * 110 / 100);
    press(&mut shop, party, Alternate);
    press(&mut shop, party, Accept);
    press(&mut shop, party, Accept);
    assert_eq!(
        party.gald, 99_999_999,
        "sale proceeds saturate at the currency limit"
    );
    assert!(!party.items.contains_key(&id));
    press(&mut shop, party, Cancel);
    press(&mut shop, party, Right);
    assert_eq!(shop.choice, Choice::Equip);
    press(&mut shop, party, Accept);
    for _ in 0..10 {
        shop.step(Default::default(), party).unwrap();
    }
    assert!(shop.take_equipment_request());
    assert!(!shop.closed);
    shop.return_from_equipment();
    for _ in 0..10 {
        shop.step(Default::default(), party).unwrap();
    }
    assert_eq!(shop.focus, Focus::Root);
    events.cancel();
    assert!(events.world.menu_request.is_none());
}
