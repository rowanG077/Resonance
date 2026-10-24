# Model node constants

Known monsters and figurines use the catalogue names listed in
[the standard library index](../README.md). Other modules are named after their
original source file and member, for example `resource_map_elc_d13_elc_d13_bin_19_0`.
Open a module to see its `Source owner` comment and the node strings it exports.
Modules without a known source owner use their actual node selectors and node
count; a stable variant suffix distinguishes otherwise identical names.

These are maintained names. Changing a model's bytes does not rename its module.
The `Skeleton` comment lets the importer find the constants for an ordered node
interface; it is not a runtime checksum or an import path. There are no aliases
for the former hash-named modules.

A mod can replace a module at its existing `scripts/std/nodes/` path and restart.
For a new model, ordinary string constants or literal node names work without an
entry in this directory.
