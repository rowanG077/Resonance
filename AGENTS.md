# Repository rules

Never create or depend on symbolic links, including in tests and development tools.
Use regular files and directories so workflows do not require Windows symlink support.
Keep checks that reject symbolic links in content.

Prefer existing tests and focused manual verification. Add tests only for a material
regression risk that existing coverage cannot verify; do not add tests for every change.
