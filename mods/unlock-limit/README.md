# Unlock Limit

An example item mod. Unlock Limit is a battle item that puts one ally into
Over Limit at once. Only one party member is in Over Limit at a time: used
while another ally is, the target is left fully charged and enters as soon as
that ally's Over Limit ends. It cannot be used on an ally already in Over Limit.

Belteon, the item shop in Meltokio, sells it for 3000 Gald. Meltokio is where
the party's options widen in the second half of the game, and the price keeps a
guaranteed Over Limit an occasional choice.

Enable it by copying or linking this directory into `local/mods`, for example
as `local/mods/000220-unlock-limit`, or pass `--mod mods/unlock-limit`.

## Files

| File | Purpose |
| --- | --- |
| `mod.json` | Name and the cooked layout this mod targets. |
| `game/items/items/unlock-limit/definition.json` | Category, base value and the `over_limit` battle use. Shops charge twice the base value. |
| `game/items/items/unlock-limit/text.json` | Name and menu captions. |
| `ui/menu/sprites/rects/item_images/unlock-limit.json` | Icon: Energy Tablets' cell of the shared item atlas. |
| `game/menu/world-map/shops/belteon/items/unlock-limit/` | One stock line in Belteon, after its original goods. |
