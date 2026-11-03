#!/usr/bin/env python3
"""Sparse original-GC enemy references; authored row selection is explicit assistance."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'oracle'))
import enemy_artes
from battle_fixture import enemy_package, neutral_movie, prepare_entry, DTM_BUTTONS
from state import State, inspect, digest
import source_worker
from source_worker import VisualWorker, write_context


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def expand(data):
    """The original wrapper, matching importer compression::decode bounds."""
    method, packed, size = struct.unpack_from('<BII', data)
    if not 0 < size <= 16 * 1024 * 1024 or 9 + packed > len(data):
        raise ValueError('invalid source package extent')
    data = iter(data[9:9 + packed])
    def take():
        try:
            return next(data)
        except StopIteration as error:
            raise ValueError('truncated source compression token') from error
    if method == 0:
        value = bytes(data)
        if len(value) != size:
            raise ValueError('stored package size differs')
        return value
    if method not in (1, 3):
        raise ValueError('unsupported source compression')
    window, initialized = bytearray(4096), bytearray(4096)
    position = 4078 if method == 1 else 4079
    initialized[:position] = b'\1' * position
    for value in range(256):
        for offset in (0, 2, 4, 6):
            window[value * 8 + offset] = value
        window[2048 + value * 7:2055 + value * 7] = bytes((value, 255, value, 255, value, 255, value))
    output, flags = bytearray(), 0
    while True:
        flags >>= 1
        if not flags & 0x100:
            flag = next(data, None)
            if flag is None:
                break
            flags = flag | 0xff00
        first = next(data, None)
        if first is None:
            break
        repeated, start, length = first, 0, 1
        if not flags & 1:
            second = take()
            start, length, repeated = first | ((second & 0xf0) << 4), (second & 15) + 3, None
            if method == 3 and length == 18:
                length, repeated = (start + 19, take()) if start < 256 else ((start >> 8) + 3, start & 255)
        if len(output) + length > size:
            raise ValueError('source package token exceeds expanded size')
        for index in range(length):
            at = (start + index) & 4095
            if repeated is None and not initialized[at]:
                raise ValueError('source package reads uninitialized dictionary')
            value = window[at] if repeated is None else repeated
            output.append(value)
            window[position], initialized[position] = value, 1
            position = (position + 1) & 4095
    if len(output) != size:
        raise ValueError('expanded source package size differs')
    return bytes(output)


def package(extracted, identity, output):
    usual = (extracted / 'files/BTL/BTLusual.dat').read_bytes()
    start, end = struct.unpack_from('>II', usual, 44)
    directory = usual[start:end]
    first, last = struct.unpack_from('>II', directory, identity * 4)
    with (extracted / 'files/BTL/BTLenemy.dat').open('rb') as stream:
        stream.seek(first)
        raw = expand(stream.read(last - first))
    if raw[:4] != b'em8\0':
        raise ValueError('invalid original enemy package')
    path = output / f'{identity:03}.bin'
    path.write_bytes(raw)
    return raw, {'path': str(path), 'sha256': digest(path), 'native_archive_range': [first, last]}


def native_requirements(raw, offset):
    """Only the source flags needed for pre-entry rank and HP fixture grants."""
    flags = struct.unpack_from('>I', raw, offset + 8)[0]
    return {'flags': flags,
            'hp_percent': 25 if flags & 1 else 50 if flags & 2 else 75 if flags & 0x100000 else None,
            'difficulty': [2 if flags & 8 else 1 if flags & 4 else 0, 0 if flags & 0x20000000 else 2]}


def replay(directory):
    """Recreate a removed accepted per-row state from its retained owner baseline."""
    directory = Path(directory).resolve()
    if not directory.is_relative_to(Path('local/battle-capture-work').resolve()):
        raise ValueError('replay belongs under local/battle-capture-work')
    recipe = read(directory / 'ready-replay.json')
    baseline = directory / recipe['baseline']
    if digest(baseline) != recipe['baseline_sha256']:
        raise ValueError('source owner baseline changed')
    state = State(baseline)
    for address, fmt, before, after in recipe['changes']:
        if state.read(int(address, 16), fmt) != before:
            raise ValueError('source typed change preimage differs')
        state.write(int(address, 16), fmt, after)
    target = directory / 'ready.s01'
    if target.exists():
        if digest(target) != recipe['state_sha256']:
            raise ValueError('existing ready state differs; refusing overwrite')
    else:
        state.save(target)
    if digest(target) != recipe['state_sha256']:
        raise ValueError('recreated ready state differs')
    movie = recipe['movie']
    companion = Path(str(target) + '.dtm')
    if not companion.exists():
        neutral_movie(baseline, companion, movie['cursor'], DTM_BUTTONS['x'] if movie['guard'] else 0, polls=movie['polls'])
    if digest(companion) != movie['sha256']:
        raise ValueError('recreated ready movie differs')
    print(json.dumps({'replayed': str(target), 'state_sha256': digest(target), 'movie_sha256': digest(companion)}))


def capture_row(worker, output, library, raw, cases, enemy, base, baseline, initialized, record, slot, package_address, index, row, result):
    began = time.monotonic()
    result['stage'] = 'preparation'
    case = copy.deepcopy(cases[enemy, row['binding']['row']])
    destination = output / row['id'].replace(':', '-')
    destination.mkdir()
    # Keep the declared initial distance inside the native admission
    # range. A zero upper bound is unbounded in the source row format.
    minimum, maximum = struct.unpack_from('>hh', raw[enemy], case['row_offset'] + 16)
    distance = max(300, minimum + 100) if minimum > 0 else 300
    if maximum > 0:
        distance = min(distance, maximum)
    if distance < minimum:
        raise ValueError('native action has no valid declared fixture distance')
    requirements = native_requirements(raw[enemy], case['row_offset'])
    threshold = requirements['hp_percent']
    hp_percent = max(1, threshold - 1) if threshold else 100
    ready = enemy_artes.prepare_row(base, destination, {**record, 'enemy_x': distance,
                                    'enemy_hp_percent': hp_percent}, initialized, case)
    proof = read(destination / 'fixture.json')
    write(destination / 'source-range.json', {'source_row_offset': case['row_offset'],
          'native_range': [minimum, maximum], 'declared_initial_distance': distance,
          'scope': 'Original signed row+16/+18 admission range; no selection requirement changed.'})
    write(destination / 'source-requirements.json', {
        'native_flags': hex(requirements['flags']), 'native_difficulty': requirements['difficulty'],
        'hp_threshold': threshold, 'declared_hp_percent': hp_percent,
        'scope': 'Choose vitals below the authored HP threshold before the first combat choice; requirements unchanged.',
    })
    ready_state = State(ready)
    write(destination / 'ready-replay.json', {
        'baseline': '../baseline.s01', 'baseline_sha256': digest(baseline),
        'changes': proof['changes'], 'state_sha256': digest(ready),
        'movie': {'baseline': '../baseline.s01.dtm', 'cursor': inspect(base)['movie']['input_count'],
                  'guard': True, 'polls': 12000, 'sha256': digest(Path(str(ready) + '.dtm'))},
        'scope': 'Apply these typed data changes to the retained owner baseline; regenerate bounded neutral guard input.',
    })
    write(destination / 'ready.json', {'battle': inspect(ready_state, battle=True)['battle']})
    write_context(destination, {'party': [1], 'case': {}, 'source_case_id': row['id'], 'acting_slot': slot,
                               'fixture_assistance': 'Authored row weight selected before first combat choice; neutral companions, raised party HP and guard; native action/physics retained.'}, library)
    result['replay'] = str((destination / 'ready-replay.json').relative_to(output))
    result['stage'] = 'admission'
    worker.load(ready)
    worker.step()
    first_sample = worker.watcher.rows
    worker.run_until(lambda observation: observation.get(f'battle_actor_{slot}_action', 0) >> 24 == 5
                     and enemy_artes.matching_metadata(proof, observation), budget=1500)
    with (worker.output / 'memory.jsonl').open() as stream:
        admitted = next(observation['battle_presentation_tick_word'] for line in stream
                        if (observation := json.loads(line))['vi_sample'] >= first_sample
                        and observation.get(f'battle_actor_{slot}_action', 0) >> 24 == 5
                        and enemy_artes.matching_metadata(proof, observation))
    # Suppress weighted reselection; ordinary native fallback remains.
    result.update(admitted=True, action_start_clock=admitted)
    worker.snapshot(destination / 'admitted.s01')
    live = State(destination / 'admitted.s01')
    live.write(package_address + case['row_offset'], 'B', 0)
    live.save(destination / 'one-action.s01')
    shutil.copyfile(str(destination / 'admitted.s01') + '.dtm', str(destination / 'one-action.s01') + '.dtm')
    write(destination / 'selection-stop.json', {'changes': live.changes, 'scope': 'Suppress weighted reselection after actual native admission; admitted action and ordinary native fallback remain unchanged.'})
    worker.load(destination / 'one-action.s01')
    worker.step()
    samples = []
    result['images'] = samples
    release = admitted
    if case['kind'] == 'common_spell':
        result['stage'] = 'release'
        sample = worker.image(destination / 'admission.png')
        samples.append({'age': sample['image_observation']['battle_presentation_tick_word'] - admitted,
                        'phase': 'admission', 'path': str(Path(sample['path']).relative_to(output)), 'sha256': sample['sha256']})
        worker.run_until(lambda observation: observation.get(f'battle_actor_{slot}_primary_phase_age_word', 0) >> 16 == 0x140)
        release = worker.watcher.latest['battle_presentation_tick_word']
    result['stage'] = 'phase images'
    for age in (0, 4, 12, 24, 40, 64, 96, 160):
        worker.run_until(lambda observation: observation['battle_presentation_tick_word'] >= release + age)
        sample = worker.image(destination / f'phase-{age:03}.png')
        samples.append({'age': sample['image_observation']['battle_presentation_tick_word'] - admitted,
                        'phase': f'phase-{age}', 'path': str(Path(sample['path']).relative_to(output)), 'sha256': sample['sha256']})
    result['stage'] = 'cleanup'
    worker.run_until(lambda observation: observation.get(f'battle_actor_{slot}_action', 0) >> 24 in (3, 8))
    sample = worker.image(destination / 'cleanup.png')
    samples.append({'age': sample['image_observation']['battle_presentation_tick_word'] - admitted,
                    'phase': 'cleanup', 'path': str(Path(sample['path']).relative_to(output)), 'sha256': sample['sha256']})
    result.update(status='captured', stage='complete', admitted=True, action_start_clock=admitted, images=samples,
                  age_origin='First observed source action5 with exact authored row/animation/command identity',
                  source_action_proof=str((destination / 'fixture.json').relative_to(output)),
                  limitation='Later context/cleanup images can include ordinary fallback after the admitted action. No visual acceptance inferred.',
                  fixture=str((destination / 'visual-fixture.json').relative_to(output)), source_provenance='source-provenance.json')
    for name in ['admitted.s01', 'one-action.s01'] + (['ready.s01'] if index else []):
        for suffix in ('', '.dtm'):
            (destination / (name + suffix)).unlink(missing_ok=True)
    result['seconds'] = time.monotonic() - began


def run_owner(args, shared):
    catalogue, provenance = read(args.catalogue), read(args.reference / 'provenance.json')
    selected = [row for row in catalogue['cases'] if row['edition'] == 'gamecube' and row['category'] == 'enemy'
                and row['binding']['enemy'] == args.enemy and (not args.case or row['id'] in args.case)]
    if not selected:
        raise ValueError('no current enemy bindings selected')
    output = args.output.resolve()
    if not output.is_relative_to(Path('local/battle-capture-work').resolve()):
        raise ValueError('source images belong under local/battle-capture-work')
    output.mkdir(parents=True, exist_ok=False)
    library = Path(catalogue['libraries']['gamecube'])
    state_path = args.reference / 'boot-field.s01'
    if digest(state_path) != provenance['boot_state_sha256'] or digest(Path(str(state_path) + '.dtm')) != provenance['boot_state_movie_sha256']:
        raise ValueError('fresh source state/movie changed')
    tree = args.reference / provenance['extracted_tree_manifest']
    if digest(tree) != provenance['extracted_tree_sha256']:
        raise ValueError('source tree manifest changed')
    disc = Path(provenance['source_executable'])
    if digest(disc) != provenance['source_dol_sha256']:
        raise ValueError('original executable changed')
    legacy = read(Path(__file__).resolve().parents[1] / 'oracle/cases/enemy-artes.json')
    cases = {(row['enemy'], row['action_row']): row for row in legacy['cases']}
    first = cases[args.enemy, selected[0]['binding']['row']]
    formation, arena = first['formation'], first['arena']
    formations = read(library / 'battle/formations.json')
    formation_row = formations['formations'][formations['mappings'][str(formation)]]
    mapping = read(library / 'battle/enemies/mappings.json')
    ids = {value: int(key) for key, value in mapping.items()}
    roster = [ids[row['enemy']] for row in formation_row['actors'] for _ in range(row.get('count', 1))]
    if args.enemy not in roster:
        raise ValueError('old source encounter does not contain requested current enemy')
    packages = output / 'packages'
    packages.mkdir()
    raw, pins = {}, {}
    for identity in set(roster):
        raw[identity], pins[identity] = package(disc.parent.parent, identity, packages)
    for row in selected:
        case = cases[args.enemy, row['binding']['row']]
        if hashlib.sha256(raw[args.enemy][case['row_offset']:case['row_offset'] + 68]).hexdigest() != case['row_sha256']:
            raise ValueError('legacy row metadata differs from actual extracted source')
    source = State(state_path)
    intervals = [native_requirements(raw[args.enemy], cases[args.enemy, row['binding']['row']]['row_offset'])['difficulty']
                 for row in selected]
    lower, upper = max(value[0] for value in intervals), min(value[1] for value in intervals)
    if lower > upper:
        raise ValueError('Selected rows require separate source difficulty baselines')
    session = source.read(0x8035a768)
    address = session + enemy_artes.BATTLE_RANK_OFFSET
    previous = source.read(address, 'B')
    rank = max(lower, min(previous, upper))
    source.write(address, 'B', rank)
    write(output / 'difficulty-fixture.json', {'source_row_intervals': intervals,
          'address': hex(address), 'before': previous, 'selected': rank,
          'scope': 'Native row+8 difficulty flags select session rank before ordinary encounter initialization.'})
    fixture = prepare_entry(source, output / 'entry.s01', inspect(source), formation, arena, party=(1,))
    neutral_movie(state_path, output / 'input.dtm', inspect(source)['movie']['input_count'], DTM_BUTTONS['x'])
    shutil.copyfile(output / 'input.dtm', output / 'entry.s01.dtm')
    report = {'source_kind': 'dolphin', 'worker_closed': False, 'simultaneous_dolphin_limit': 1,
              'cases': [{'id': row['id'], 'binding': row['binding'], 'status': 'pending', 'visual_review': 'pending'} for row in selected]}
    write(output / 'source-provenance.json', {**provenance, 'source_reference': str(args.reference),
          'catalogue_sha256': digest(args.catalogue), 'packages': pins,
          'scope': 'Original GC executable, current enemy binding, actual source row; controlled selection is assistance, not natural admission or parity.'})
    os.environ.pop('LD_LIBRARY_PATH', None)
    os.environ['RESONANCE_DOLPHIN'] = provenance['dolphin']
    expected_owner_slot = 4 + roster.index(args.enemy)
    source_worker.MINIMAL.update({f'battle_actor_{slot}_action' for slot in [0] + list(range(4, 4 + len(roster)))})
    source_worker.MINIMAL.add(f'battle_actor_{expected_owner_slot}_primary_phase_age_word')
    source_worker.MINIMAL.update({f'enemy_{slot}_{field}' for slot in [expected_owner_slot]
                                 for field in ('animation_pointer', 'command_pointer', 'native_word', 'selected_row_pointer')})
    worker = shared[0]
    try:
        if worker is None:
            worker = VisualWorker(str(disc), output / 'input.dtm', output / 'entry.s01', output / 'worker',
                                  fixture['marker_address'], enemy_artes.locations(), disc_sha256=digest(tree))
            shared[0] = worker
        report['pid'] = worker.process.pid
        print(json.dumps({'worker_pid': worker.process.pid}), flush=True)
        entry = output / 'entry.s01'
        for attempt in range(9):
            worker.warmup(entry, output / f'entry-{attempt}', fixture['case_marker'], 180 if attempt == 0 else 30)
            worker.snapshot(output / 'initialized.s01')
            base = State(output / 'initialized.s01')
            battle = inspect(base, battle=True)['battle']
            if battle['combat_tick']:
                raise ValueError('missed first source enemy selection')
            if battle['actors'] and all(actor['action'][0] == 13 for actor in battle['actors']):
                break
            entry = output / 'initialized.s01'
        else:
            raise ValueError('source enemy entry did not initialize')
        actual_packages = {identity: enemy_package(base, data) for identity, data in raw.items()}
        package_address = actual_packages[args.enemy][0]
        table = package_address + struct.unpack_from('>H', raw[args.enemy], 10)[0]
        candidates = [actor for actor in battle['actors'] if actor['side'] == 1 and base.read(actor['address'] + 0x50) == table]
        if not candidates:
            raise ValueError('requested original enemy action-table binding absent')
        owner = candidates[0]
        if owner['slot'] != expected_owner_slot:
            raise ValueError('actual source owner differs from the minimally observed slot')
        for actor in battle['actors']:
            if actor['slot'] != owner['slot']:
                address = actor['address']
                base.write(address + 0x107c, 'B', base.read(address + 0x107c, 'B') & ~0x38)
                base.write(address + 0x107d, 'B', (base.read(address + 0x107d, 'B') & 0x3f) | (0 if actor['slot'] == 0 else 0xc0))
        write(output / 'neutral-companions.json', {'changes': base.changes, 'owner_slot': owner['slot']})
        baseline = output / 'baseline.s01'
        base.save(baseline)
        shutil.copyfile(str(output / 'initialized.s01') + '.dtm', str(baseline) + '.dtm')
        write(output / 'baseline-provenance.json', {'initialized_sha256': digest(output / 'initialized.s01'),
              'baseline_sha256': digest(baseline), 'changes': base.changes,
              'scope': 'Owner baseline differs from observed preselection checkpoint only by these declared neutral controls.'})
        initialized = {'battle': battle, 'actor': owner, 'packages': {str(key): {'address': value[0], 'proof': value[1]}
                       for key, value in actual_packages.items()}, 'settled_idle': False}
        record = {'packages': [{'enemy': key, 'package': pin} for key, pin in pins.items()],
                  'enemy': {'enemy': args.enemy, 'package': pins[args.enemy]}, 'enemy_x': 300,
                  'enemy_hp_percent': 100, 'guard': True, 'initialized_state': str(baseline),
                  'movie_polls': 12000}
        slot = owner['slot']
        for index, (row, result) in enumerate(zip(selected, report['cases'])):
            began = time.monotonic()
            try:
                capture_row(worker, output, library, raw, cases, args.enemy, base, baseline,
                            initialized, record, slot, package_address, index, row, result)
            except (ValueError, RuntimeError) as error:
                if isinstance(error, RuntimeError) and 'guest-frame budget' not in str(error):
                    raise
                result.update(reason=str(error), seconds=time.monotonic() - began)
                destination = output / row['id'].replace(':', '-')
                if index and (destination / 'ready-replay.json').exists():
                    for name in ('ready.s01', 'admitted.s01', 'one-action.s01'):
                        for suffix in ('', '.dtm'):
                            (destination / (name + suffix)).unlink(missing_ok=True)
            write(output / 'nativecapture-report.json', report)
            print(json.dumps({'id': row['id'], 'status': result['status'],
                              'seconds': result.get('seconds'), 'reason': result.get('reason')}), flush=True)
    except Exception as error:
        report['error'] = str(error)
        if 'result' in locals():
            result.setdefault('reason', str(error))
        raise
    finally:
        if (output / 'baseline-provenance.json').exists():
            for suffix in ('', '.dtm'):
                (output / ('initialized.s01' + suffix)).unlink(missing_ok=True)
        report['worker_closed'] = False
        report['owner_finished'] = True
        report['captured'] = sum(row['status'] == 'captured' for row in report['cases'])
        write(output / 'nativecapture-report.json', report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalogue', type=Path, required=True)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--enemy', type=int, action='append', default=[])
    parser.add_argument('--case', action='append', default=[])
    parser.add_argument('--plan', type=Path, help='Bounded list of current case IDs under cases[].id')
    args = parser.parse_args()
    catalogue = read(args.catalogue)
    if args.plan:
        args.case = [row['id'] for row in read(args.plan)['cases']]
    selected = [row for row in catalogue['cases'] if row['edition'] == 'gamecube' and row['category'] == 'enemy'
                and (not args.enemy or row['binding']['enemy'] in args.enemy)
                and (not args.case or row['id'] in args.case)]
    if not selected or not (args.enemy or args.case):
        raise ValueError('select a bounded enemy or case scope')
    if args.case and set(args.case) - {row['id'] for row in selected}:
        raise ValueError('requested current enemy case is absent')
    enemies = list(dict.fromkeys(row['binding']['enemy'] for row in selected))
    # The fixed observer contains only slots needed by this bounded plan.
    legacy = read(Path(__file__).resolve().parents[1] / 'oracle/cases/enemy-artes.json')
    native = {(row['enemy'], row['action_row']): row for row in legacy['cases']}
    library = Path(catalogue['libraries']['gamecube'])
    formations = read(library / 'battle/formations.json')
    ids = {value: int(key) for key, value in read(library / 'battle/enemies/mappings.json').items()}
    for enemy in enemies:
        row = next(row for row in selected if row['binding']['enemy'] == enemy)
        fixture = native[enemy, row['binding']['row']]
        formation = formations['formations'][formations['mappings'][str(fixture['formation'])]]
        roster = [ids[row['enemy']] for row in formation['actors'] for _ in range(row.get('count', 1))]
        slot = 4 + roster.index(enemy)
        source_worker.MINIMAL.update({f'battle_actor_{index}_action' for index in [0] + list(range(4, 4 + len(roster)))})
        source_worker.MINIMAL.add(f'battle_actor_{slot}_primary_phase_age_word')
        source_worker.MINIMAL.update({f'enemy_{slot}_{field}' for field in
                                     ('animation_pointer', 'command_pointer', 'native_word', 'selected_row_pointer')})
    batch = len(enemies) > 1
    root = args.output.resolve()
    if batch:
        if not root.is_relative_to(Path('local/battle-capture-work').resolve()):
            raise ValueError('batch belongs under local/battle-capture-work')
        root.mkdir(parents=True, exist_ok=False)
    case_results = {row['id']: {'id': row['id'], 'binding': row['binding'], 'status': 'pending',
                               'visual_review': 'pending'} for row in selected}
    report = {'source_kind': 'dolphin', 'simultaneous_dolphin_limit': 1,
              'worker_closed': False, 'cases': list(case_results.values()), 'owners': []}
    shared, owner_reports = [None], []
    began = time.monotonic()
    infrastructure_error = False
    try:
        for enemy in enemies:
            owner = argparse.Namespace(**vars(args))
            owner.enemy = enemy
            owner.case = [row['id'] for row in selected if row['binding']['enemy'] == enemy]
            owner.output = root / f'owner-{enemy:03}' if batch else root
            owner_error = None
            try:
                run_owner(owner, shared)
            except (ValueError, RuntimeError) as error:
                if isinstance(error, RuntimeError) and 'guest-frame budget' not in str(error):
                    raise
                owner_error = str(error)
                print(json.dumps({'owner': enemy, 'status': 'pending', 'reason': str(error)}), flush=True)
            path = owner.output / 'nativecapture-report.json'
            if path.exists():
                result = read(path)
            else:
                result = {'source_kind': 'dolphin', 'worker_closed': False,
                          'cases': [{'id': row['id'], 'binding': row['binding'], 'status': 'pending',
                                      'reason': owner_error or 'Source owner preparation failed', 'visual_review': 'pending'}
                                     for row in selected if row['binding']['enemy'] == enemy]}
                write(path, result)
            owner_reports.append(path)
            report['owners'].append({'enemy': enemy, 'report': str(path.relative_to(root)) if batch else path.name})
            for row in result['cases']:
                value = copy.deepcopy(row)
                if batch:
                    prefix = owner.output.relative_to(root)
                    for key in ('fixture', 'source_action_proof', 'source_provenance', 'replay'):
                        if key in value:
                            value[key] = str(prefix / value[key])
                    for image in value.get('images', []):
                        image['path'] = str(prefix / image['path'])
                case_results[value['id']] = value
            report['cases'] = list(case_results.values())
            if batch:
                report.update(captured=sum(row['status'] == 'captured' for row in report['cases']),
                              pending=sum(row['status'] == 'pending' for row in report['cases']),
                              seconds=time.monotonic() - began)
                write(root / 'nativecapture-report.json', report)
    except Exception as error:
        infrastructure_error = True
        report['error'] = str(error)
        raise
    finally:
        if shared[0]:
            report['pid'] = shared[0].process.pid
            shared[0].close(complete=not infrastructure_error)
        for path in owner_reports:
            if path.exists():
                owner = read(path)
                owner['worker_closed'] = True
                write(path, owner)
        if not batch and owner_reports and owner_reports[0].exists():
            report = {**read(owner_reports[0]), 'seconds': time.monotonic() - began}
        report['worker_closed'] = True
        report.update(captured=sum(row['status'] == 'captured' for row in report['cases']),
                      pending=sum(row['status'] == 'pending' for row in report['cases']),
                      seconds=time.monotonic() - began)
        write(root / 'nativecapture-report.json', report)
        print(json.dumps({key: report.get(key) for key in ('worker_closed', 'pid', 'captured', 'pending', 'seconds', 'error')}), flush=True)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == 'replay':
        replay(sys.argv[2])
    else:
        main()
