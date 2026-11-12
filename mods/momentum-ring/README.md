# Momentum Ring

An example equipment mod. The Momentum Ring is an accessory any character can
wear. Each hit already in the combo on a target raises the wearer's damage to
that target by 1%, up to 20%: the first hit is unchanged, the second deals 1%
more, and from the twenty-first on every hit deals 20% more. The bonus ends
when the combo does. It applies to normal attacks, artes and spells alike, but
not to the hits of a Unison Attack.

Fighting Spirit, the armory in Luin, sells it for 6000 Gald.

Enable it by copying or linking this directory into `local/mods`, for example
as `local/mods/000230-momentum-ring`, or pass `--mod mods/momentum-ring`.

## Files

| File | Purpose |
| --- | --- |
| `mod.json` | Name and the cooked layout this mod targets. |
| `game/items/items/momentum-ring/definition.json` | Ring category, who can wear it, base value and the `combo_damage` property. Shops charge twice the base value. |
| `game/items/items/momentum-ring/text.json` | Name and menu captions. |
| `game/items/equipment_descriptions/momentum-combo-damage.json` | The line shown for the ring on the status page. |
| `ui/menu/sprites/rects/item_images/momentum-ring.json` | Icon: Attack Ring's cell of the shared item atlas. |
| `game/menu/world-map/shops/fighting-spirit/items/momentum-ring/` | One stock line in Fighting Spirit, after its original goods. |

`combo_damage` takes any `per_hit` and `limit` percentages. When a character
wears several pieces with the property, the strongest one counts.
