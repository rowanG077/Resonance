# Changelog

## 0.3.0 - *04 Nov, 2026*

### Added:
- Press Z (Unison / Taunt) in the battle command menu to cycle player one's character through Auto slots. Other players keep their characters, and the new party order persists after battle.
- Press Start in the battle command menu to show the opening character's control mode above their portrait, then press it again to cycle Manual, Semi-Auto and Auto. Changes are retained when closing the selector.
  The Artes header now shows the mode-switching hint.
- Implemented material-specific footstep sounds while walking and running in fields and on the world map.
- Session logs and crash reports now capture errors, backtraces and recent game context, with native crash dumps when available. Send the session folder to the team so crashes can be investigated using the matching release symbols.
  [P44](https://github.com/rowanG077/Resonance/pull/44)
- Added an OpenGL graphics backend, selectable in Customize. Unsupported MSAA and SMAA S2x settings fall back to SMAA Ultra.
- Added an immediately applied, saved VSync option alongside the graphics settings.

### Changed:
- Release CI automatically consolidates and archives changelog entries, commits the versioned notes to the release branch, and uses them as the GitHub release notes.
  [P44](https://github.com/rowanG077/Resonance/pull/44)

### Fixed:
- Airborne attacks now preserve jump momentum and match the GameCube's action timing.
- Player control now resumes immediately after the battle opening camera pan, removing the extra delay before movement and actions.
- Fixed log session creation on Windows and crash-monitor cleanup on macOS when a dump cannot be created.
  [P44](https://github.com/rowanG077/Resonance/pull/44)
- Fixed overlapping battle notices, Technical Smash text layering, and Artes menu highlights covering text.
- Battle entry now warms the crack effect with the scene's antialiasing and color format, preventing a brief white screen while a different graphics pipeline compiles.
- Battle menus now draw over the paused battle scene and HUD, fixing black backgrounds and old menu artwork showing through other menus.
- Battles entered directly from an overworld save now prepare their own shading texture, preventing characters from appearing white.
- Reduced camera and animation stutters under CPU contention by removing busy-waiting parallel transform propagation.
