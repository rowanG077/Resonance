"""Data-only battle entry and capture helpers for the pinned Dolphin profile."""
import json
from hashlib import sha256
from pathlib import Path
import struct
import time

import effect_workers
from effect_fixture import prepare
from state import State, digest

DTM_HEADER_BYTES = 256
DTM_INPUT_BYTES = 8
DTM_CONNECTED = 0x4000
# Same recording bits as tools/oracle/src/main.rs::Button. These differ from
# the controller masks in initialized game data; choose a physical button.
DTM_BUTTONS = {
    "start": 1, "a": 2, "b": 4, "x": 8, "y": 16, "z": 32,
    "up": 64, "down": 128, "left": 256, "right": 512, "l": 1024, "r": 2048,
}
ENEMY_ACTION_BYTES = 68
RAM_BASE = 0x80000000
PARTY_SETTINGS_POINTER = 0x8035A768
PARTY_ORDER_OFFSET = 0xE9D
PARTY_CONTROLS_OFFSET = 0xEA5
LLOYD_STATS_OFFSET = 0x2B8
LLOYD_HP_OFFSETS = (0x12, 0x26, 0x36)
PARTY_MEMBER_BYTES = 0x118
JOIN_CHARACTER_COMMAND = 0x24
START_BATTLE_COMMAND = 0xCD
OBSERVATION_POLL_VIS = 30


class _BattleValidationError(RuntimeError):
    pass


class BattleFixture(effect_workers.DolphinFixture):
    """Use data markers and consecutive VIs without executable memory watches."""

    def __init__(self, *args, **kwargs):
        watcher = effect_workers.Watcher

        def data_watcher(user, log, locations, *rest, **options):
            excluded = {"focus_patch_word", "secondary_blur_patch_word"}
            return watcher(user, log, {key: label for key, label in locations.items()
                                      if label not in excluded}, *rest, **options)

        # One worker is constructed at a time within each capture process.
        effect_workers.Watcher = data_watcher
        try:
            super().__init__(*args, **kwargs)
        finally:
            effect_workers.Watcher = watcher

    @staticmethod
    def validate_rows(rows, expected):
        for index, row in enumerate(rows):
            if row["case_marker"] != expected:
                raise _BattleValidationError("Dolphin did not load the requested battle fixture")
            if index and row["presentation_counter"] != rows[index - 1]["presentation_counter"] + 1:
                raise _BattleValidationError("nonconsecutive battle presentation")

    def _capture(self, method, state, target, expected, count, *, minimum_progress=None, **options):
        target = Path(target)
        existed = target.exists()
        first_row = self.watcher.rows
        try:
            return method(state, target, expected, count, **options)
        except Exception as error:
            validation_failed = isinstance(error, _BattleValidationError) or (
                not existed and (target / "registration-failure.json").is_file())
            if (validation_failed and self.paused and self.process.poll() is None
                    and self.watcher.error is None
                    and self.watcher.rows - first_row >= (count if minimum_progress is None else minimum_progress)):
                retained = target / "validation-failure"
                receipt = {"operation": method.__name__, "error": str(error), "error_type": type(error).__name__,
                           "input_state": str(state), "expected_marker": expected,
                           "observed_marker": self.watcher.latest.get("case_marker"),
                           "first_vi_sample": first_row, "observations_advanced": self.watcher.rows - first_row,
                           "terminal_observation": dict(self.watcher.latest),
                           "marker_verified": False, "capture_accepted": False, "images_registered": False,
                           "scope": "Unverified live terminal state; fixture identity and observations require review."}
                # Exclusive output keeps input and accepted checkpoints intact.
                created = False
                try:
                    retained.mkdir()
                    created = True
                    receipt["input_state_sha256"] = digest(state)
                    self.snapshot(retained / "terminal.s01")
                    receipt["state_sha256"] = digest(retained / "terminal.s01")
                    receipt["movie_sha256"] = digest(retained / "terminal.s01.dtm")
                    receipt["state_retained"] = True
                except Exception as salvage_error:
                    receipt.update(state_retained=False, salvage_error=str(salvage_error))
                self.metadata.setdefault("validation_failures", []).append({"path": str(retained), **receipt})
                if created:
                    try:
                        (retained / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
                    except OSError as receipt_error:
                        error.add_note(f"Could not write salvage provenance: {receipt_error}")
            raise

    @staticmethod
    def label_capture(target):
        path = Path(target) / "capture.json"
        capture = json.loads(path.read_text())
        presentation = capture.setdefault("presentation", {})
        presentation.pop("gecko_verified", None)
        presentation["profile_configured_not_memory_verified"] = True
        path.write_text(json.dumps(capture, indent=2) + "\n")

    def sequence(self, state, target, expected, count, **options):
        result = self._capture(super().sequence, state, target, expected, count, **options)
        self.label_capture(target)
        return result

    def observe(self, state, target, expected, count):
        result = self._capture(super().observe, state, target, expected, count)
        self.label_capture(target)
        return result

    def observe_until(self, state, target, expected, max_count, done):
        """Collect numeric observations until done(rows) or the requested limit.

        Load once and pause only at the end. Retain the final paused tail, which
        can exceed max_count; the receipt distinguishes the decision from that
        actual endpoint. A completion predicate is not a fidelity assertion.
        """
        return self._capture(self._observe_until, state, target, expected, max_count,
                             done=done, minimum_progress=1)

    def _observe_until(self, state, target, expected, max_count, *, done):
        if self.capture_images or max_count <= 0 or not callable(done):
            raise ValueError("observe_until requires numeric capture, a positive limit and a predicate")
        target.mkdir()
        self.load(state)
        first_row = self.watcher.rows
        rows = []
        started = time.monotonic()
        with (self.output / "memory.jsonl").open() as observations:
            observations.seek(0, 2)

            def collect():
                # Watcher increments its row count only after flushing the line.
                available = self.watcher.rows - first_row
                batch = [json.loads(observations.readline()) for _ in range(available - len(rows))]
                if any(row["vi_sample"] != first_row + len(rows) + index for index, row in enumerate(batch)):
                    raise _BattleValidationError("nonconsecutive observation records")
                self.validate_rows(rows[-1:] + batch, expected)
                rows.extend(batch)

            self.pause()
            try:
                while True:
                    self.wait_observations(first_row, min(max_count, len(rows) + OBSERVATION_POLL_VIS))
                    collect()
                    if done(rows):
                        reason = "predicate"
                        break
                    if len(rows) >= max_count:
                        reason = "max_count"
                        break
                decision_count = len(rows)
            except Exception as error:
                if not self.paused:
                    try:
                        self.pause()
                    except Exception as pause_error:
                        error.add_note(f"Could not pause failed observation: {pause_error}")
                raise
            self.pause()
            collect()
        (target / "observations.json").write_text(json.dumps(rows) + "\n")
        sample = {"state_sha256": digest(state), "observations": len(rows),
                  "max_count": max_count, "completion_reason": reason,
                  "decision_observations": decision_count, "seconds": time.monotonic() - started,
                  "first_vi_sample": first_row,
                  "observations_sha256": digest(target / "observations.json")}
        self.metadata["samples"].append(sample)
        (target / "capture.json").write_text(json.dumps({**self.metadata, "samples": [sample],
            "complete": True, "presentation": {"profile_configured_not_memory_verified": True, "images": False},
            "audio": {"backend": "No Audio Output"}}, indent=2) + "\n")
        return rows

    def warmup(self, state, target, expected, count):
        return self._capture(super().warmup, state, target, expected, count)


def neutral_movie(source, target, cursor, button=0, *, polls=12000):
    """Append bounded input using DTM_BUTTONS, never game controller masks."""
    if cursor < 0 or polls <= 0 or not 0 <= button < DTM_CONNECTED:
        raise ValueError("invalid movie cursor, poll count, or controller buttons")
    data = bytearray(Path(str(source) + ".dtm").read_bytes())
    end = DTM_HEADER_BYTES + cursor * DTM_INPUT_BYTES
    if data[:4] != b"DTM\x1a" or len(data) < end:
        raise ValueError("checkpoint movie is invalid or its history is truncated")
    data = data[:end]
    data.extend(struct.pack("<H6B", DTM_CONNECTED | button, 0, 0, 128, 128, 128, 128) * polls)
    data[12] = 1  # Resume from a checkpoint.
    for offset in (13, 21):
        struct.pack_into("<Q", data, offset, cursor + polls)
    struct.pack_into("<Q", data, 237, 2**64 - 1)
    target = Path(target)
    target.write_bytes(data)
    effect_workers.set_movie_backend(target)


def prepare_entry(source, target, observation, formation, arena, *, seed=1, hp=20000, party=(1,)):
    """Enter an authored encounter with a declared manual party and raised HP.

    Enemy selection, action commands, and battle positions are left authored.
    The source movie is copied alongside the prepared checkpoint; callers can
    replace it with ``neutral_movie`` before launching the fixture.
    """
    if not 1 <= hp <= 65535:
        raise ValueError("fixture HP must fit the party stat fields")
    if (not 1 <= len(party) <= 4 or len(set(party)) != len(party) or
            any(character not in range(1, 10) for character in party)):
        raise ValueError("fixture party must contain one to four distinct characters")
    target = Path(target)
    commands = ([(JOIN_CHARACTER_COMMAND, [character]) for character in party]
                if tuple(party) != (1,) else [])
    commands.append((START_BATTLE_COMMAND, [formation, arena, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0]))
    fixture = prepare(source, target, observation, [], commands, seed=seed)
    state = State(target)
    settings = state.read(PARTY_SETTINGS_POINTER)
    for slot in range(8 if len(party) > 1 else 4):
        state.write(settings + PARTY_ORDER_OFFSET + slot, "B", party[slot] if slot < len(party) else 0)
    for slot in range(4):
        state.write(settings + PARTY_CONTROLS_OFFSET + slot, "B", 0)
    for character in party:
        member = settings + LLOYD_STATS_OFFSET + (character - 1) * PARTY_MEMBER_BYTES
        for offset in LLOYD_HP_OFFSETS:
            state.write(member + offset, "H", hp)
    state.save(target)
    fixture.update(party_changes=state.changes, fixture_sha256=digest(target), declared_party=list(party))
    return fixture


def unique_enemy_package(state, raw):
    """Locate one exact authored action table; this alone does not bind an actor.

    Use ``battle_identity.EnemyCatalogue.identify`` for initialized actors.
    That helper preserves ambiguous profile matches, including enemies 236/237.
    """
    if len(raw) < 488 or raw[:4] != b"em8\0":
        raise ValueError("invalid authored enemy package")
    start = struct.unpack_from(">H", raw, 10)[0]
    offsets = [struct.unpack_from(">H", raw, i)[0] for i in range(4, 20, 2)]
    offsets += [struct.unpack_from(">I", raw, i)[0] for i in range(24, 488, 4)]
    end = min((offset for offset in offsets if start < offset < len(raw)), default=len(raw))
    actions = raw[start:end]
    if start < 488 or len(actions) < ENEMY_ACTION_BYTES:
        raise ValueError("authored enemy action table is missing or truncated")
    memory = bytes(state.ram)
    offset = memory.find(actions)
    if offset < start or memory.find(actions, offset + 1) >= 0:
        raise ValueError("authored enemy action table is absent or not unique in live data")
    return RAM_BASE + offset - start


def enemy_package(state, raw):
    """Locate an exact header and six complete authored sections.

    Encounter initialization can change vital names and action rows. Those two
    sections are excluded explicitly. Staged enemy initialization narrows its
    policy to the first five actions; that typed rewrite is recorded separately.
    Actor identity still needs its own initialized-data observation.
    """
    header_bytes = 488
    if len(raw) < header_bytes or raw[:4] != b"em8\0":
        raise ValueError("invalid authored enemy package")
    memory = bytes(state.ram)
    offset = memory.find(raw[:header_bytes])
    if offset < 0 or memory.find(raw[:header_bytes], offset + 1) >= 0:
        raise ValueError("authored enemy package header is absent or not unique")
    offsets = [struct.unpack_from(">H", raw, field)[0] for field in range(4, 20, 2)]
    offsets += [struct.unpack_from(">I", raw, field)[0] for field in range(24, header_bytes, 4)]
    matched = [{"name": "header", "offset": 0, "bytes": header_bytes,
                "sha256": sha256(raw[:header_bytes]).hexdigest()}]
    for field, name in ((4, "profile"), (8, "hit_rules"), (12, "policy"),
                        (14, "commands"), (16, "contacts"), (18, "animations")):
        start = struct.unpack_from(">H", raw, field)[0]
        if not header_bytes <= start < len(raw):
            raise ValueError(f"invalid authored enemy {name} section")
        end = min((value for value in offsets if start < value < len(raw)), default=len(raw))
        section = raw[start:end]
        initialized = memory[offset + start:offset + end]
        initialization = None
        if (name == "policy" and len(section) >= 24 and section[8] in (7, 17)
                and section[22:24] == bytes((0, 9)) and initialized != section):
            initial_stage = bytearray(section)
            initial_stage[23] = 5
            if initialized == initial_stage:
                initialization = {"kind": "transformation" if section[8] == 7 else "health_stage",
                                  "action_range": [0, 5],
                                  "authored_action_range": [0, 9]}
        if initialized != section and initialization is None:
            raise ValueError(f"initialized enemy {name} differs from its authored package")
        matched.append({"name": name, "header_field": field, "offset": start,
                        "bytes": len(section), "sha256": sha256(section).hexdigest()})
        if initialization:
            matched[-1].update(initialization=initialization,
                               initialized_sha256=sha256(initialized).hexdigest())
    address = RAM_BASE + offset
    return address, {"package_address": address, "source_sha256": sha256(raw).hexdigest(),
                     "basis": "unique exact header and six complete sections with recorded initialization",
                     "matched": matched, "excluded_sections": [
                         {"header_field": 6, "reason": "encounter initialization changes displayed vital names"},
                         {"header_field": 10, "reason": "encounter initialization changes action rows"}]}
