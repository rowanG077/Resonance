# All-Multiply

An example item mod. All-Multiply is a battle item that doubles all damage dealt
by allies and enemies until the battle ends: the counterpart of All-Divide.
Using both in one battle cancels out.

- Halo, the Iselia shop, sells it for 500 Gald.
- Every defeated Fake has a 15% chance to drop one, in addition to its
  original drops.

Enable it by copying or linking this directory into `local/mods`, for example
as `local/mods/000200-all-multiply`, or pass `--mod mods/all-multiply`.

## Files

| File | Purpose |
| --- | --- |
| `mod.json` | Name and the cooked layout this mod targets. |
| `game/items/items/all-multiply/definition.json` | Category, base value and the `all_multiply` battle use. Shops charge twice the base value. |
| `game/items/items/all-multiply/text.json` | Name and menu captions. |
| `ui/menu/sprites/rects/item_images/all-multiply.json` | Icon: All-Divide's cell of the shared item atlas. |
| `game/menu/world-map/shops/halo/items/all-multiply/` | One stock line in Halo, after its original goods. |
| `battle/rewards/monsters/73/all-multiply.json` | The drop from Fake, Monster List entry 73. |

The item has no number. The game assigns one at startup and saves record it by
name, so this mod does not conflict with other mods that add items.
