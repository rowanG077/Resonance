# Changelog

In `nix develop`, run `qlog entry` to add a change and `qlog check --all` to validate entries.
Preview release notes with `qlog collect --version X.Y.Z`.
Release CI archives pending entries under `releases/vX.Y.Z/entries/`, writes `releases/vX.Y.Z.md`, and pushes a bot commit to the release branch. CI then tests and publishes that commit using the consolidated notes.
