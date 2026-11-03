"""One original-GC worker with sparse screenshots and minimal admission data."""
import json
from pathlib import Path
import shutil
import struct
import subprocess
import time

from battle_fixture import BattleFixture, DTM_BUTTONS, neutral_movie
from capture import press_key
import effect_workers
from state import digest
from state import State, inspect
import battle_artes


MINIMAL = {'case_marker', 'presentation_counter', 'requested_native_word',
           'battle_presentation_tick_word', 'battle_combat_tick_word',
           'battle_actor_0_action', 'battle_actor_0_primary_phase_age_word',
           'battle_actor_0_tp_word'}


class VisualWorker(BattleFixture):
    def __init__(self, *args, **kwargs):
        original = effect_workers.Watcher

        def minimal(user, log, locations, *rest, **options):
            locations = {address: name for address, name in locations.items()
                         if name in MINIMAL or name.startswith('projectile_')
                         and (name.endswith('_address') or name.endswith('_28'))}
            locations['8035a628'] = 'presentation_counter'
            return original(user, log, locations, *rest, include_defaults=False, **options)

        effect_workers.Watcher = minimal
        options = effect_workers.DOLPHIN_OPTIONS
        effect_workers.DOLPHIN_OPTIONS = options + ['-C', 'Dolphin.Core.EmulationSpeed=0.0']
        try:
            super().__init__(*args, capture_images=False, **kwargs)
        finally:
            effect_workers.Watcher = original
            effect_workers.DOLPHIN_OPTIONS = options
        self.metadata['visual_capture'] = 'Sparse F9 screenshots; all-frame dumping disabled'
        windows = subprocess.check_output(
            ['xdotool', 'search', '--onlyvisible', '--pid', str(self.process.pid)],
            env=self.env, text=True).splitlines()
        subprocess.run(['xdotool', 'windowfocus', '--sync', windows[-1]], env=self.env, check=True)

    def drain(self):
        # There is no all-frame dump queue. Only settle the small observer log;
        # each requested PNG has its own completion/registration check below.
        previous, stable = self.watcher.rows, time.monotonic()
        deadline = stable + 5
        while time.monotonic() < deadline:
            current = self.watcher.rows
            if current != previous:
                previous, stable = current, time.monotonic()
            elif time.monotonic() - stable >= .08:
                return
            time.sleep(.005)
        raise TimeoutError('source observer did not settle after pausing')

    def pause(self):
        # The full-trace helper holds F10 for150ms. At uncapped speed that can
        # skip an entire short arte before returning. Release on acknowledgement
        # when resuming, and use a short own-window pulse when pausing.
        before = self.watcher.rows
        subprocess.run(['xdotool', 'keydown', 'F10'], env=self.env, check=True)
        try:
            if self.paused:
                deadline = time.monotonic() + 1
                while self.watcher.rows == before and time.monotonic() < deadline:
                    time.sleep(.002)
            else:
                time.sleep(.035)
        finally:
            subprocess.run(['xdotool', 'keyup', 'F10'], env=self.env, check=True)
        self.paused = not self.paused
        if self.paused:
            self.drain()

    def run_until(self, predicate, *, budget=1500):
        """Run to an observed milestone, then pause; retain the actual endpoint."""
        start = self.watcher.latest.get('battle_presentation_tick_word', 0)
        if predicate(self.watcher.latest):
            return
        self.pause()
        deadline, previous = time.monotonic() + 30, self.watcher.rows
        try:
            while not predicate(self.watcher.latest):
                if self.process.poll() is not None or self.watcher.error:
                    raise RuntimeError('source worker stopped: ' + str(self.watcher.error))
                if self.watcher.latest.get('battle_presentation_tick_word', 0) - start > budget:
                    raise RuntimeError('source visual milestone exceeded its guest-frame budget')
                if self.watcher.rows != previous:
                    previous, deadline = self.watcher.rows, time.monotonic() + 30
                elif time.monotonic() > deadline:
                    raise TimeoutError('source visual worker stopped producing observations')
                time.sleep(.002)
        finally:
            if not self.paused:
                self.pause()

    def image(self, target):
        """Request the paused frame; two acknowledged VIs flush Dolphin's readback."""
        if not self.paused:
            raise RuntimeError('pause before requesting a source image')
        directory = self.user / 'ScreenShots'
        before = set(directory.rglob('*.png')) if directory.exists() else set()
        previous = dict(self.watcher.latest)
        press_key(self.env, 'F9')
        self.step()
        observed = dict(self.watcher.latest)
        self.step()
        deadline = time.monotonic() + 10
        while True:
            images = set(directory.rglob('*.png')) - before
            if images:
                if len(images) != 1:
                    raise RuntimeError('one screenshot request produced multiple images')
                source = images.pop()
                try:
                    # PNG is written asynchronously; its IEND must be complete.
                    if source.read_bytes()[-12:] == b'\0\0\0\0IEND\xaeB`\x82':
                        shutil.copyfile(source, target)
                        break
                except OSError:
                    pass
            if time.monotonic() > deadline:
                raise TimeoutError('source screenshot did not finish')
            time.sleep(.02)
        return {'path': str(target), 'sha256': digest(target),
                'before': previous, 'image_observation': observed, 'after': dict(self.watcher.latest),
                'registration': 'F9 request for paused source frame; two acknowledged VIs flush its readback'}


def ready_state(worker, output, record):
    """Declare manual opponents during entry; let their normal entry finish."""
    output = Path(output)
    marker = record['fixture']['case_marker']
    source = output / 'entry.s01'
    initialized = output / 'initialized.s01'
    for attempt in range(9):
        worker.warmup(source, output / f'entry-{attempt}', marker, 180 if attempt == 0 else 30)
        worker.snapshot(initialized)
        state = State(initialized)
        try:
            battle = inspect(state, battle=True)['battle']
        except ValueError:
            source = initialized
            continue
        if battle['combat_tick']:
            raise RuntimeError('source visual setup missed the first opponent choice')
        if battle['actors'] and all(actor['action'][0] == 13 or
                                   actor['hp'] == 0 and actor['action'][0] == 18
                                   for actor in battle['actors']):
            break
        source = initialized
    else:
        raise RuntimeError('source visual entry did not initialize')
    # Existing arte_equivalence.manual_opponents data recipe. No action, timer,
    # animation or physics lane is forced, and the original game code is intact.
    for actor in battle['actors']:
        if actor['slot']:
            address = actor['address']
            state.write(address + 0x107c, 'B', state.read(address + 0x107c, 'B') & ~0x38)
            state.write(address + 0x107d, 'B', (state.read(address + 0x107d, 'B') & 0x3f) | 0xc0)
    neutral = output / 'neutral-entry.s01'
    state.save(neutral)
    shutil.copyfile(str(initialized) + '.dtm', str(neutral) + '.dtm')
    (output / 'neutral-opponents.json').write_text(json.dumps({
        'changes': state.changes, 'scope': 'Declared neutral manual opponents before their first AI choice; ordinary entry and physics retained.'}, indent=2) + '\n')
    worker.warmup(neutral, output / 'settled-entry', marker, 300)
    worker.snapshot(output / 'warm.s01')
    ready = battle_artes.prepare_ready(output, record)
    if record['case'].get('input') == 'aerial':
        # Observed native121 admission from this ready-relative physical input.
        # Keep the source's mode0; the diagnostic's mode1 attempt stayed grounded.
        cursor = inspect(State(ready))['movie']['input_count']
        temporary = output / 'aerial-input.dtm'
        neutral_movie(ready, temporary, cursor, polls=12000)
        movie = bytearray(temporary.read_bytes())
        for poll in range(cursor + 20, cursor + 80):
            movie[256 + poll * 8 + 5] = 255
        for poll in range(cursor + 88, cursor + 92):
            struct.pack_into('<H', movie, 256 + poll * 8, 0x4000 | DTM_BUTTONS['b'])
        companion = Path(str(ready) + '.dtm')
        companion.unlink()
        companion.write_bytes(movie)
        temporary.unlink()
        (output / 'aerial-input.json').write_text(json.dumps({
            'source_kind': 'dolphin', 'cursor': cursor, 'up_polls': [20, 80],
            'b_polls': [88, 92], 'source_control_mode': 0,
            'movie_sha256': digest(companion),
            'scope': 'Ready-relative ordinary input proven by native121 admission; no action, pose, or physics injection.',
            'diagnostic': 'gc-original-aerial-input-probe-20261103/experiment.json',
        }, indent=2) + '\n')
    if record['case']['fixture'] == 'summon':
        # Saved OverLimit is already an explicit summon fixture grant. Neutral
        # entry consumes some of its duration; retain a full ordinary duration
        # for the future B input, without inventing or forcing an activation.
        state = State(ready)
        actor = inspect(state, battle=True)['battle']['actors'][0]['address']
        mode = state.read(actor + 0x107d, 'B') & 7
        if mode == 4:
            before = state.read(actor + 0x10a8, 'h')
            state.write(actor + 0x10a8, 'h', 1000)
            state.save(ready)
            assistance = {'native_mode': mode, 'remaining_before': before, 'remaining_after': 1000,
                          'changes': state.changes,
                          'scope': 'Refresh the already active OverLimit duration for the declared summon input.'}
            (output / 'active-overlimit.json').write_text(json.dumps(assistance, indent=2) + '\n')
    return ready


def capture(worker, output, record, ready):
    output = Path(output)
    began = time.monotonic()
    worker.load(ready)
    worker.step()  # Publish the loaded state's data before evaluating admission.
    if worker.watcher.latest.get('case_marker') != record['fixture']['case_marker']:
        raise RuntimeError('source visual fixture marker differs')
    caster = json.loads((output / 'ready.json').read_text())['battle']['actors'][0]['address']
    native = record['case']['native']
    expected = record['defaults']['tp'] // 10 if record['case'].get('input') == 'guard_down' else record['case']['tp_cost']
    start_row = worker.watcher.rows
    samples = []

    def image(label):
        sample = worker.image(output / f'{len(samples):02}-{label}.png')
        sample['phase'] = label
        samples.append(sample)

    def clock(row):
        return row.get('battle_presentation_tick_word', 0)

    worker.run_until(lambda row: row.get('requested_native_word', 0) & 65535 == native
                     and row.get('battle_actor_0_tp_word', 0) & 65535 == record['defaults']['tp'] - expected)
    observed_admission = clock(worker.watcher.latest)
    # Keep the first requested identity from the complete, small observer log;
    # the host can pause a few VIs after noticing it.
    with (worker.output / 'memory.jsonl').open() as stream:
        admission = next((clock(row) for line in stream if (row := json.loads(line))['vi_sample'] >= start_row
                          and row.get('requested_native_word', 0) & 65535 == native), observed_admission)
    image('admission')
    if record['case']['fixture'] != 'martial':
        worker.run_until(lambda row: row.get('battle_actor_0_primary_phase_age_word', 0) >> 16 == 0x140)
        release = clock(worker.watcher.latest)
        image('release')
    else:
        release = admission
    # Summon presentations continue past the initial casting circle. Retain two
    # later phases so the reference includes the spirit, within the sparse cap.
    ages = (12, 20, 28, 40, 64, 128, 256) if record['case']['fixture'] == 'summon' else (12, 20, 28, 40, 64)
    for age in ages:
        worker.run_until(lambda row: clock(row) >= release + age)
        image(f'phase-{age}')
    # Charge releases through the ordinary recovery action while retaining its
    # 0x142 scene marker. The actor's recovery/idle transition is authoritative
    # for that source path as well as ordinary martial actions.
    worker.run_until(lambda row: row.get('battle_actor_0_action', 0) >> 24 in (0, 3, 8)
                     or row.get('battle_actor_0_primary_phase_age_word', 0) >> 16 in (0x200, 0x240))
    recovered = clock(worker.watcher.latest)
    image('recovery')

    def owned(row):
        return any(row.get(f'projectile_{slot}_address') and row.get(f'projectile_{slot}_28') == caster
                   for slot in range(16))

    worker.run_until(lambda row: clock(row) >= recovered + 40 and not owned(row))
    image('cleanup')
    for sample in samples:
        sample['age'] = clock(sample['image_observation']) - admission
    receipt = {'source_kind': 'dolphin', 'admitted': True, 'native': native,
               'ready_state_sha256': digest(ready),
               'ready_movie_sha256': digest(Path(str(ready) + '.dtm')),
               'expected_tp_payment': expected, 'admission_clock': admission,
               'recovery_clock': recovered, 'samples': samples,
               'observer_locations': worker.watcher.locations,
               'first_vi_sample': start_row, 'last_vi_sample': worker.watcher.rows,
               'seconds': time.monotonic() - began,
               'scope': 'Sparse original-game visual references; no damage or parity comparison.'}
    (output / 'visual-capture.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return receipt


def write_context(output, record, library):
    """Small matched-renderer context observed from the retained ready state."""
    output, library = Path(output), Path(library)
    battle = json.loads((output / 'ready.json').read_text())['battle']
    state = State(output / 'ready.s01')
    formations = json.loads((library / 'battle/formations.json').read_text())
    formation = formations['formations'][formations['mappings'][str(battle['formation'])]]
    enemies = {name: int(number) for number, name in
               json.loads((library / 'battle/enemies/mappings.json').read_text()).items()}
    enemy_ids = [enemies.get(row['enemy']) for row in formation['actors'] for _ in range(row.get('count', 1))]
    actors = []
    for actor in battle['actors']:
        value = {key: actor[key] for key in ('slot', 'side', 'position', 'heading', 'center',
                                            'center_offset', 'model_scale', 'action', 'hp', 'max_hp', 'tp', 'max_tp')}
        slot = actor['slot']
        value['logical_id'] = record['party'][slot] if slot < 4 else (
            enemy_ids[slot - 4] if slot - 4 < len(enemy_ids) else None)
        address = actor['address']
        value['motion'] = {key: state.read(address + offset, 'f') for key, offset in
                           [('forward_step', 0x1908), ('forward', 0x1910), ('vertical', 0x1914),
                            ('acceleration', 0x1918), ('gravity', 0x191c)]}
        value['motion']['floor_armed'] = bool(state.read(address + 0x1183, 'B') & 4)
        value['overlimit_active'] = state.read(address + 0x107d, 'B') & 7 == 4
        value['overlimit_timer_or_charge'] = state.read(address + 0x10a8, 'h')
        actors.append(value)
    case = record['case']
    document = {'source_kind': 'dolphin',
                'source_case_id': record.get('source_case_id', f"gamecube:party:{case.get('character')}:{case.get('arte')}"),
                'formation': battle['formation'], 'arena': battle['arena'],
                'difficulty': state.read(state.read(0x8035a768) + 0x19f, 'B'),
                'party': record['party'], 'enemies': enemy_ids, 'acting_slot': record.get('acting_slot', 0),
                'actors': actors, 'camera': battle['camera'],
                'fixture_assistance': record.get('fixture_assistance', 'Neutral manual opponents declared during entry; original entry and physics settled.'),
                'ready_state_sha256': digest(output / 'ready.s01')}
    (output / 'visual-fixture.json').write_text(json.dumps(document, indent=2) + '\n')
    return document
