These scenarios use ordinary keyboard or controller input. Run `field-route.json`
from the maintained post-Frank school-grounds save at the activated memory circle
(map 332, story 2500). It walks to the village and back; the quicksave probe's
`--route` option also checks that returning reuses the prepared field resources.
The classroom transition remains covered by the field lifecycle tests.

`new-game.json` starts from the title and reaches the first playable field.
Loading is allowed to finish at native speed; waits measure gameplay updates.

`field-menus.json` uses a free-control school-grounds save with an Apple Gel in
the inventory. It captures Main, Items, the item target panel, cancellation back
to the list, and Equipment. `items_focus` requires the requested List or Target
state before `menu_settled` observes entrance slides and selection
trails before capture; input remains immediate. Ordinary taps need only one
press update and one release update, without fixed animation padding. Decorative
cursor bobs and blinking markers continue normally.
