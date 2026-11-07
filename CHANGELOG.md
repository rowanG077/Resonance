# Changelog

## 0.4.0 - *07 Nov, 2026*

### Fixed:
- Learning an arte now lets the acquisition announcement finish before playing the arte callout, so neither spoken line is cut off or lost. Cancelled or superseded callouts are discarded before playback. Queued callouts are also cleared on defeat, even when no defeat voice is available. [P72](https://github.com/rowanG077/Resonance/pull/72)
- Normal weapon attacks now activate their sword trails using the original contact slot and ribbon lifetime. [P70](https://github.com/rowanG077/Resonance/pull/70)
  Contact-driven trails also support carried weapon slots above three.
- Queued battle arte commands now display a pulsing RESERVE. label over the ordered character's portrait and vitals until execution or cancellation. Battle font validation checks the label's glyphs before the HUD starts, preventing delayed rendering failures with incomplete fonts. [P63](https://github.com/rowanG077/Resonance/pull/63)
- Attempting an arte without enough TP now shows a blinking TP XX warning above the character. HUD setup checks the required glyphs so incomplete fonts fail before combat. [P62](https://github.com/rowanG077/Resonance/pull/62)
- Field ambience, including burning Iselia's fire, resumes correctly after returning from battle even when its sound sources have not moved. [P59](https://github.com/rowanG077/Resonance/pull/59)
- Page Up and Page Down now navigate the Synopsis list and text, and page through Save/Load slots alongside controller shoulder buttons. Save/Load paging keeps the final six slots within the selected bank, preventing invalid rows and a crash at the end of bank B. [P60](https://github.com/rowanG077/Resonance/pull/60)
- The title screen now highlights Continue when a player save slot exists, including when returning to the title. Selection depends only on the file's presence, without validating its contents. Title input replays and recording scenarios with input retain their New Game starting selection; observation-only captures preserve the player's default. The development `--selected` override has been removed. [P61](https://github.com/rowanG077/Resonance/pull/61)
- Fixed seams and stray atlas pixels in tiled battle notices and menu artwork when antialiasing is enabled, including dark joins in linearly filtered artwork. [P73](https://github.com/rowanG077/Resonance/pull/73)
- Upcoming, unlearned artes now appear blue on the Artes combo-binding page, matching the original technique list. [P64](https://github.com/rowanG077/Resonance/pull/64)
- Restored scripted head and neck gestures during battle events, including Kratos's arrival and the party's reactions in the Vidarr fight. [P58](https://github.com/rowanG077/Resonance/pull/58)
- Consecutive scripted encounters, including Vidarr after the temple soldiers, preserve the covered field during loading, preventing a second screen shatter. [P65](https://github.com/rowanG077/Resonance/pull/65)
- Overworld characters and Noishe now use running animations above half maximum movement speed, with matching footsteps and footprints. [P74](https://github.com/rowanG077/Resonance/pull/74)
- Fixed DirectX 12 morph-target indexing for meshes drawn from a shared vertex buffer. [P57](https://github.com/rowanG077/Resonance/pull/57)
- Using Guardian through an assigned arte shortcut in Manual or Semi-Auto mode now activates the defensive stance instead of ending the battle with an error. [P62](https://github.com/rowanG077/Resonance/pull/62)
- Fixed hair and cloth stretching or snapping when combat resumes after a scene. [P58](https://github.com/rowanG077/Resonance/pull/58)
- Restored distance fog on DirectX 12 across field, battle, and overworld scenes, including burning Iselia's red sky after Exbelua. [P57](https://github.com/rowanG077/Resonance/pull/57)
- Restored the original left-to-right black wipe and timing when battle results close, after defeats, and when scripted enemy retreats interrupt combat, including both early Yggdrasill battles. [P66](https://github.com/rowanG077/Resonance/pull/66)
- Restored the fade from black into battles, including covered scripted encounters without event scenes and Vidarr after the temple soldiers. The cover stays visible during loading and on the first activation frame. [P69](https://github.com/rowanG077/Resonance/pull/69)
- Fixed holes and seams around the sword badge in battle technique notices by aligning its frame and background to the same origin. [P73](https://github.com/rowanG077/Resonance/pull/73)
- Restored scene dissolves during battle handoffs, including entering and leaving Vidarr's rescue sequence and summon introductions during boss battles. [P67](https://github.com/rowanG077/Resonance/pull/67)
- Restored Mithos's half-health conversation with party-dependent replies and Abyssion's speech at 75% health. Lines preserve their timing and speakers while other voices become quieter. Normal volume and guard settings return when the conversation or combat ends; Abyssion's restrictions end after requesting his final line. Canceling or storing a spell preserves ongoing speech. [P68](https://github.com/rowanG077/Resonance/pull/68)
- Full-screen fades and shaded overlays now cover the aligned viewport completely, preventing thin strips of the scene from remaining visible along the right and bottom edges with MSAA. [P73](https://github.com/rowanG077/Resonance/pull/73)
- Scripted battles entered from a white screen, including Mithos's Rainbow Angel form, now fade into battle from white instead of switching to black at activation. [P71](https://github.com/rowanG077/Resonance/pull/71)

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
