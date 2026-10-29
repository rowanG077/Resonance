# Symphonia constants

This leaf crate owns known numeric game identities. One `ids!` declaration supplies
an explicit `#[repr(...)]` enum, iteration, checked numeric conversion, symbolic
parsing/printing and optional initial resource names. It depends only on optional
Serde support, not the importer, runtime or script compiler.

```rust
use symphonia_constants::{FieldId, ItemId};
assert_eq!(ItemId::MemoryGem.raw(), 491);
assert_eq!(ItemId::try_from(491), Ok(ItemId::MemoryGem));
assert_eq!(ItemId::MemoryGem.to_string(), "MemoryGem");
assert_eq!(FieldId::Iselia0332.resource_name(), Some("iselia-0332"));
```

`resonance-script-content` generates `.sym` definitions from `KnownId` during
cooking and for development checks. Add or correct IDs here, not in generated
script files or parallel importer name tables. Derived subsets such as playable
party characters and caravan stops reference the original enum's discriminants.

Enum names are stable symbols. Resource names are initial editable asset paths;
localized captions are separate data. Unknown or mod-added IDs remain raw numeric
values at asset/script boundaries; failed `TryFrom` does not make them invalid
resources. Edition adapters may translate source IDs into these common identities.

Only established story flag names and unresolved values used by behavior are
listed. Numeric gaps do not get invented placeholder identities. Model node names
are strings from model data; `MODEL_NODES` records only reviewed semantic aliases.
