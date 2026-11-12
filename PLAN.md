# Battle simplification

The current work reduces implementation and test complexity while preserving
supported gameplay. Use direct behavior-oriented designs and meaningful names.
Keep binary format knowledge at the import boundary. Source-code provenance,
function addresses, and historical implementation layouts do not belong in the
maintained engine or its tests.

The engine now uses native movement and recovery state, typed action timelines,
one prepared technique capacity, shared asset inventories, and event-based replay.
Gameplay randomness is separate from cosmetic effects. Further restructuring
should produce a smaller design with a concrete behavior check.

Preserve the fixes for Colette's victory grip, Genis's facial texture selection,
and the Iselia exit prompt. The unimplemented overworld may still return to title.
Keep fast prepared-asset loading and the default tolerant error policy;
`--paranoid` remains available for validation.

Validate the resulting behavior directly. Run focused
component tests, formatting, Clippy for affected crates, and representative
asset-backed/GPU routes. Compare gameplay at matching events and report
loading-related audio differences separately. Historical reports describe their
captured builds and must not be presented as current validation.

See [battle support and validation](docs/battle-status.md) and the
[capture guide](tools/oracle/README.md) for current support, limits, and commands.
