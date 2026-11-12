# Unison Force

An example item mod. Unison Force is a battle item that fills the party's
Unison gauge at once. It has no effect before Unison Attacks are available.

Marble's, the item shop in Palmacosta, sells it for 1200 Gald. Palmacosta is
the first large town after the party learns Unison Attacks, and the price keeps
it a deliberate purchase for a difficult battle rather than a routine one.

Enable it by copying or linking this directory into `local/mods`, for example
as `local/mods/000210-unison-force`, or pass `--mod mods/unison-force`.

## Files

| File | Purpose |
| --- | --- |
| `mod.json` | Name and the cooked layout this mod targets. |
| `game/items/items/unison-force/definition.json` | Category, base value and the `unison_gauge` battle use. Shops charge twice the base value. |
| `game/items/items/unison-force/text.json` | Name and menu captions. |
| `ui/menu/sprites/rects/item_images/unison-force.json` | Icon: Rune Bottle's cell of the shared item atlas. |
| `game/menu/world-map/shops/marble-s/items/unison-force/` | One stock line in Marble's, after its original goods. |
