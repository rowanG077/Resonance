# Changelog

## 0.5.0 - *13 Nov, 2026*

### Added:
- Load mods as ordered layers over the base game: later layers override matching files, while independent named content combines across mods. Numbered directories in `local/mods` load automatically; explicit `--mod` directories take precedence. Restart the game after changing mods.
  [P76](https://github.com/rowanG077/Resonance/pull/76)
- Added event skipping. Press F8 to fast-forward scripted scenes while preserving
  story progress. Skipping stops at choices and battles, leaving encounters to be
  played normally. Reassign or clear Skip Event in Customize → Controls for
  keyboard and controller input.
  [P99](https://github.com/rowanG077/Resonance/pull/99)
- F1 now toggles fullscreen, returning to the last fullscreen mode used this
  session and preserving the selected resolution. The shortcut can be reassigned
  or cleared in Customize → Controls and does not change saved display preferences.
  [P99](https://github.com/rowanG077/Resonance/pull/99)
- Customize → Controls now exposes Performance Overlay (F3), Save Performance
  Snapshot (F4), Double Speed (F6), and Pause / Resume (F7).
  These controls can be reassigned or cleared for keyboard and controller input.
  Pause shows an animated prompt for the configured resume control.
  [P99](https://github.com/rowanG077/Resonance/pull/99)

### Changed:
- Textures now use BC7 for direct GPU upload. **Recook the complete asset library before running this version.** Devices without BC7 support decode textures to RGBA.
  [P76](https://github.com/rowanG077/Resonance/pull/76)
- Precache shared resources during startup and reuse them across fields, the
  overworld, and battles to reduce loading pauses.
  [P78](https://github.com/rowanG077/Resonance/pull/78)

### Fixed:
- Fixed seams in scaled menu and battle artwork with SMAA and other antialiasing modes.
  [P82](https://github.com/rowanG077/Resonance/pull/82)
- Battle stick directions now register at moderate tilts, so jumping while moving no longer needs a precise full diagonal.
  [P87](https://github.com/rowanG077/Resonance/pull/87)
- Successful escapes now let active attacks and jumps finish before characters run away, while preventing further attack chains.
  [P85](https://github.com/rowanG077/Resonance/pull/85)
- Successful escapes now show the Escaped banner with the party leader's portrait.
  [P86](https://github.com/rowanG077/Resonance/pull/86)
- Limited Ghost Wolf's dash distance to prevent Lloyd from crossing the entire arena toward distant enemies.
  [P79](https://github.com/rowanG077/Resonance/pull/79)
- Restored item statistics in Equip, Items, Collector's Book, and World Map shop lists.
  [P81](https://github.com/rowanG077/Resonance/pull/81)
- Field and overworld movement now follows the analog stick linearly, and walk
  and run animations keep pace with the ground instead of sliding.
  [P80](https://github.com/rowanG077/Resonance/pull/80)
- Page Up and Page Down now scroll the main-menu party list, Display Change, Exchange and character selection, including while choosing an exchange target.
  [P88](https://github.com/rowanG077/Resonance/pull/88)
- Enemies stunned by the Sorcerer's Ring now keep animating while their movement is paused.
  [P83](https://github.com/rowanG077/Resonance/pull/83)
- Fixed a false missing-model warning when using the Sorcerer's Ring.
  [P83](https://github.com/rowanG077/Resonance/pull/83)
- Fixed Sword Dancer's face lighting.
  [P84](https://github.com/rowanG077/Resonance/pull/84)
- Fixed Colette's backstep movement and recovery after she acquires her wings.
  [P93](https://github.com/rowanG077/Resonance/pull/93)
- Fixed villagers sliding away in their reaction poses after Lloyd's banishment from Iselia.
  [P96](https://github.com/rowanG077/Resonance/pull/96)
- Party and enemy action notices now remain visible while the Magic Lens panel is open.
  [P89](https://github.com/rowanG077/Resonance/pull/89)
- Fixed missing Monster List models, sideways carried objects, and models hidden behind menu artwork.
  [P94](https://github.com/rowanG077/Resonance/pull/94)
- Noishe now dismounts automatically when entering a region where riding has not been unlocked.
  [P92](https://github.com/rowanG077/Resonance/pull/92)
- Fixed floating character shadows during the boat's arrival in Palmacosta.
  [P95](https://github.com/rowanG077/Resonance/pull/95)
- Technical Smash notices no longer overlap battle menu pages.
  [P97](https://github.com/rowanG077/Resonance/pull/97)
- Fixed field models loading another room's data, including Tower of Mana chests appearing as mirrors.
  [P98](https://github.com/rowanG077/Resonance/pull/98)

### Removed:
- Removed the development dungeon menu and quicksave shortcuts.
  [P99](https://github.com/rowanG077/Resonance/pull/99)

## 0.4.0 - *07 Nov, 2026*

### Fixed:
- Learning an arte now lets the acquisition announcement finish before playing the arte callout, so neither spoken line is cut off or lost. Cancelled or superseded callouts are discarded before playback. Queued callouts are also cleared on defeat, even when no defeat voice is available. [P72](https://github.com/rowanG077/Resonance/pull/72)
- Normal weapon attacks now activate their sword trails using the original contact slot and ribbon lifetime. [P70](https://github.com/rowanG077/Resonance/pull/70)
  Contact-driven trails also support carried weapon slots above three.
- Queued battle arte commands now display a pulsing RESERVE. label over the ordered character's portrait and vitals until execution or cancellation. [P63](https://github.com/rowanG077/Resonance/pull/63)
- Attempting an arte without enough TP now shows a blinking TP XX warning above the character. [P62](https://github.com/rowanG077/Resonance/pull/62)
- Field ambience, including burning Iselia's fire, resumes correctly after returning from battle. [P59](https://github.com/rowanG077/Resonance/pull/59)
- Page Up and Page Down now navigate the Synopsis list and text, and page through Save/Load slots alongside controller shoulder buttons. [P60](https://github.com/rowanG077/Resonance/pull/60)
- The title screen now highlights Continue when a player save slot exists, including when returning to the title. Selection depends only on the file's presence, without validating its contents. [P61](https://github.com/rowanG077/Resonance/pull/61)
- Fixed seams and stray atlas pixels in tiled battle notices and menu artwork when antialiasing is enabled, including dark joins in linearly filtered artwork. [P73](https://github.com/rowanG077/Resonance/pull/73)
- Upcoming, unlearned artes now appear blue on the Artes combo-binding page, matching the original technique list. [P64](https://github.com/rowanG077/Resonance/pull/64)
- Restored scripted head and neck gestures during battle events. [P58](https://github.com/rowanG077/Resonance/pull/58)
- Consecutive scripted encounters, including Vidarr after the renegades, preserve the covered field during loading, preventing a second screen shatter. [P65](https://github.com/rowanG077/Resonance/pull/65)
- Overworld characters and Noishe now use running animations above half maximum movement speed, with matching footsteps and footprints. [P74](https://github.com/rowanG077/Resonance/pull/74)
- Fixed DirectX 12 morph-target indexing for meshes drawn from a shared vertex buffer. [P57](https://github.com/rowanG077/Resonance/pull/57)
- Using Guardian through an assigned arte shortcut in Manual or Semi-Auto mode now activates the defensive stance instead of ending the battle with an error. [P62](https://github.com/rowanG077/Resonance/pull/62)
- Fixed hair and cloth stretching or snapping when combat resumes after a scene. [P58](https://github.com/rowanG077/Resonance/pull/58)
- Restored distance fog on DirectX 12 across field, battle, and overworld scenes, fixes missing burning Iselia's red sky after Exbelua. [P57](https://github.com/rowanG077/Resonance/pull/57)
- Restored the original left-to-right black wipe and timing when battle results close, after defeats, and when scripted enemy retreats interrupt combat, including both early Yggdrasill battles. [P66](https://github.com/rowanG077/Resonance/pull/66)
- Restored the fade from black into battles, including covered scripted encounters without event scenes and Vidarr after the temple soldiers. [P69](https://github.com/rowanG077/Resonance/pull/69)
- Fixed holes and seams around the sword badge in battle technique notices by aligning its frame and background to the same origin. [P73](https://github.com/rowanG077/Resonance/pull/73)
- Restored scene dissolves during battle handoffs, including entering and leaving Vidarr's rescue sequence and summon introductions during boss battles. [P67](https://github.com/rowanG077/Resonance/pull/67)
- Restored Mithos's half-health conversation with party-dependent replies and Abyssion's speech at 75% health. Lines preserve their timing and speakers while other voices become quieter. [P68](https://github.com/rowanG077/Resonance/pull/68)
- Full-screen fades and shaded overlays now cover the aligned viewport completely, preventing thin strips of the scene from remaining visible along the right and bottom edges with MSAA. [P73](https://github.com/rowanG077/Resonance/pull/73)
- Scripted battles entered from a white screen, including the final battle Rainbow Angelic form, now fade into battle from white instead of switching to black at activation. [P71](https://github.com/rowanG077/Resonance/pull/71)

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
