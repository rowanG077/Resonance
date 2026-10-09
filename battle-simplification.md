# Battle architecture

The current work list and validation results live in [plan.md](plan.md). This
file describes the maintained design; completed reviews and their old proposals
remain in Git history.

## Ownership

- Importers decode assets. Preparation resolves immutable definitions and checks
  external inputs. Simulation owns gameplay; presentation owns optional artwork,
  audio, notices and rumble. Missing feedback cannot prevent a gameplay commit.
- `ActionKey` names a prepared definition, a catalogue ID names a persistent
  technique, and `ActionId` names a live execution. Learning changes membership
  and shortcuts without replacing prepared actions.
- Each actor owns its current action, payment, interruption and recovery. Released
  volleys and projectiles keep their own lifetimes after caster interruption.
- Gameplay publishes a completed update before presentation consumes it. Menus
  pause the gameplay clock. Gameplay and cosmetic randomness are separate;
  rejected transactions preserve authoritative state and randomness.
- Result application commits rewards, inventory, learning and progress once.
  Scene changes prepare a candidate before replacing the live scene.

## Implementation choices

Native actions specify timing, hit policy, geometry and reactions directly.
Ordinary attacks share an event runner; casting uses a projectile volley.
Balance values stay beside the behavior they tune. Asset identifiers remain at
resource binding boundaries.

Damage uses a direct arithmetic pipeline, without a configurable modifier
framework. Model playback owns common locomotion and recovery poses. Optional
feedback can use the action's existing event cursor, but trailing cosmetic work
cannot postpone gameplay recovery.

Prepared attacks validate contacts, throws and projectiles during the same event
walk. Field and world loading use the shared session-definition admission path.
Prepared field packages supply their text, rules and effects on entry; consumers
borrow the active scene's definitions. Skits reuse those admitted resources.
The presentation owner contains one active field or world. Saves identify that
scene explicitly; loading prepares a replacement before retiring the live scene.
World saves have no dependency on a previously visited field.
Menus retain that same scene checkpoint; save-slot summaries keep only party and
display data. World preparation extends the admitted shared inventory, preserving
its diagnostic policy. Field artwork is cached only for the current playthrough.

Internal APIs and cooked formats may change freely. There are no historical
behavior or random-sequence compatibility requirements. Do not consult, copy or
reference decompiled implementations.

## Validation and scope

Keep tests for payment, interruption, contacts, pause, deterministic replay and
atomic inventory/reward/save handoff. Content checks cover preparation and
cross-module behavior. Remove tests that only mirror copied fields, obsolete
capabilities or private scheduling details. Validate coherent batches and rerun
only affected checks after failures or further edits.

Fire Ball is the supported spell release. Additional spells, artes, enemy
variants and complete Unison execution are separate feature work. Physical
controller testing remains an accepted manual follow-up. Automated routes do
not establish subjective feel, all-character balance or audio quality.
