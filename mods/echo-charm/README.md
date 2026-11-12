# Echo Charm

An example equipment mod. The Echo Charm is an accessory any character can
wear. Each time the wearer pays for an arte or a spell, there is a 20% chance
that the next one costs 0 TP. The free use shows as a TP cost of 0 in the arte
list, an equipment-effect label appears over the wearer when it is granted,
and it is spent by the next arte or spell whatever that would have cost. A free
use can itself grant the next one. Normal attacks neither grant nor spend it.

Snow White, the accessory shop in Flanoir, sells it for 8000 Gald.

Enable it by copying or linking this directory into `local/mods`, for example
as `local/mods/000240-echo-charm`, or pass `--mod mods/echo-charm`.

## Files

| File | Purpose |
| --- | --- |
| `mod.json` | Name and the cooked layout this mod targets. |
| `game/items/items/echo-charm/definition.json` | Charm category, who can wear it, base value and the `free_arte_chance` property. Shops charge twice the base value. |
| `game/items/items/echo-charm/text.json` | Name and menu captions. |
| `game/items/equipment_descriptions/echo-free-arte.json` | The line shown for the charm on the status page. |
| `ui/menu/sprites/rects/item_images/echo-charm.json` | Icon: Drain Charm's cell of the shared item atlas. |
| `game/menu/world-map/shops/snow-white/items/echo-charm/` | One stock line in Snow White, after its original goods. |

`free_arte_chance` is a percentage. When a character wears several pieces with
the property, the highest one counts.
