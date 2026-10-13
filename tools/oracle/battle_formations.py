#!/usr/bin/env python3
"""Observe selected formation entry and enemy identities in one resident Dolphin.

Run from the repository root. Images, arte behavior and actual battle results are
separate evidence; this command never claims those from an entry observation.
"""
import argparse
from collections import Counter, defaultdict
import gzip
import json
import math
import os
from pathlib import Path
import shutil
import tempfile
import time

from battle_identity import EnemyCatalogue
from battle_fixture import BattleFixture, neutral_movie, prepare_entry
from battle_state import watch_locations
from state import State, digest, inspect

REPO = Path(__file__).resolve().parents[2]
BATTLE_MODE = 9
BATTLE_BSS_SECTION = 6
ACTOR_CAPACITY = 12


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def pinned(record):
    path = Path(record['path'])
    if digest(path) != record['sha256']:
        raise ValueError(f'fixture hash differs: {path}')
    return path


def selection(args, profile):
    formations = json.loads((args.assets / 'battle/formations.json').read_text())
    records = formations['records']
    ids = args.ids if args.ids is not None else list(range(args.range[0], args.range[1] + 1))
    if not ids or len(ids) != len(set(ids)) or any(i < 0 or i >= len(records) for i in ids):
        raise ValueError('select unique formation IDs within the cooked catalogue')
    entries = defaultdict(list)
    for row in json.loads(pinned(profile['entry_receipts']).read_text())['records']:
        if row['entry_registered']:
            entries[row['formation']].append(dict(path=row['path'], arena=row['arena'],
                actor_identity_verified=row.get('actor_identity_verified', False)))
    arenas = {}
    for call in json.loads(pinned(profile['authored_arenas']).read_text())['calls']:
        for identity in call['formations']:
            arenas.setdefault(identity, (call['arena'],
                f"field {call['map_id']}, {call['command']} at scenario word {call['command_word']}"))
    worlds = json.loads((args.assets / 'worlds/world.json').read_text())['encounters']['worlds']
    for world_index, world in enumerate(worlds):
        terrain_arenas = worlds[min(world_index, 1)]['arena_by_terrain']
        for area_index, area in enumerate(world['areas']):
            for terrain, pools in enumerate(area['encounter_groups'] if area else []):
                for pool in pools:
                    for row in formations['pools'][pool]:
                        if row['weight']:
                            arenas.setdefault(row['formation'], (terrain_arenas[terrain],
                                f'world {world_index}, area {area_index}, terrain {terrain}, pool {pool}'))
    cases, reused = [], []
    for identity in ids:
        prior = entries.get(identity, [])
        if args.missing_entry and prior:
            reused.append(dict(formation=identity, entry_evidence=prior,
                               actor_identity_verified=any(r['actor_identity_verified'] for r in prior)))
            continue
        record = records[identity]
        arena, basis = arenas.get(identity, (profile['component_arena'], 'declared component arena'))
        if prior:
            arena, basis = prior[0]['arena'], 'retained entry arena'
        if args.arena is not None:
            arena, basis = args.arena, 'explicit component arena'
        if not (args.assets / f'battle/stages/{arena:03}.json').exists():
            raise ValueError(f'arena {arena} is not cooked')
        actors = [dict(actor, enemy=record['resources'][actor['resource']]['enemy'])
                  for actor in record['actors']]
        cases.append(dict(formation=identity, arena=arena, arena_basis=basis,
                          expected_actors=actors, entry_evidence=prior))
    return dict(catalogue_size=len(records), selected_ids=ids, cases=cases,
                reused_entries=reused, formations_sha256=digest(args.assets / 'battle/formations.json'),
                scope='Entry and initialized enemy identities only; no visual, arte or result acceptance')


def snapshot(worker, target, catalogue):
    worker.snapshot(target)
    state = State(target)
    battle = inspect(state, battle=True)['battle']
    battle['state_sha256'] = state.sha256
    identities = [catalogue.identify(state, actor) for actor in battle.get('actors', [])
                  if actor['side'] == 1]
    return battle, identities


def check_entry(case, rows, battle):
    expected_scene = (case['formation'], case['arena'], BATTLE_MODE)
    if (battle['formation'], battle['arena'], battle['mode']) != expected_scene or not rows:
        raise ValueError('terminal state is not the requested battle')
    if any((r['battle_formation'], r['battle_arena'], r['global_mode_word'] & 0x7f) !=
           expected_scene for r in rows):
        raise ValueError('observed VIs include a different battle or scene')
    if battle.get('bss_index') != BATTLE_BSS_SECTION:
        raise ValueError('battle observation pointer layout changed')


def check_actors(case, battle, identities):
    if any(not all(math.isfinite(v) for v in actor['position']) for actor in battle['actors']):
        raise ValueError('non-finite live actor position')
    expected = Counter(actor['enemy'] for actor in case['expected_actors'])
    actual = Counter(actor['enemy'] for actor in identities)
    if None in actual:
        raise ValueError('enemy initialized profile identity is unmatched or ambiguous')
    if actual != expected:
        raise ValueError(f'expected enemy counts {dict(expected)}, observed {dict(actual)}')
    return dict(expected_enemy_counts=dict(expected), observed_enemy_counts=dict(actual))


def run(args, profile, selected):
    output = args.output.resolve()
    if not output.is_relative_to((REPO / 'local/battle-capture-work').resolve()):
        raise ValueError('output must be under disk-backed local/battle-capture-work')
    output.mkdir(parents=True)
    work = output / 'work'
    work.mkdir()
    write(output / 'selection.json', selected)
    source = pinned(profile['source'])
    if pinned(profile['source_movie']) != Path(str(source) + '.dtm'):
        raise ValueError('source movie must be the checkpoint companion')
    base = State(source)
    observed = inspect(base)
    movie = work / 'neutral.dtm'
    neutral_movie(source, movie, observed['movie']['input_count'])
    catalogue = EnemyCatalogue(profile['enemy_packages'], args.assets)
    write(output / 'enemy-identities.json', catalogue.records)
    locations = watch_locations(dict(pool=1, bss_index=BATTLE_BSS_SECTION,
                                    actors=[dict(slot=i) for i in range(ACTOR_CAPACITY)]))
    worker, results = None, []
    previous_tmpdir = tempfile.tempdir
    alias = tempfile.TemporaryDirectory(prefix='bf-', dir='/tmp')
    tempfile.tempdir = alias.name
    try:
        for case in selected['cases']:
            start = time.monotonic()
            retained = output / f"{case['formation']:03}-{case['arena']:02}"
            retained.mkdir()
            scratch = work / retained.name
            scratch.mkdir()
            result = dict(formation=case['formation'], arena=case['arena'], entry_registered=False,
                          actor_identity_verified=False, image_registration='not_requested',
                          visual_reviewed=False, enemy_artes_verified=False, lifecycle_verified=False)
            try:
                entry = scratch / 'entry.s01'
                fixture = prepare_entry(base, entry, observed, case['formation'], case['arena'],
                                        hp=profile['party_hp'])
                shutil.copyfile(movie, str(entry) + '.dtm')
                fixture.update(case=case, movie_sha256=digest(movie))
                with gzip.open(retained / 'fixture.json.gz', 'wt') as stream:
                    json.dump(fixture, stream)
                if worker is None:
                    worker = BattleFixture(args.disc, movie, entry, work / f'worker-{time.time_ns()}',
                        fixture['marker_address'], locations, disc_sha256=profile['disc_sha256'],
                        capture_images=False)
                worker.warmup(entry, scratch / 'warmup', fixture['case_marker'], profile['warmup_vis'])
                warm = scratch / 'warm.s01'
                worker.snapshot(warm)
                rows = worker.observe(warm, scratch / 'observations', fixture['case_marker'], profile['observations'])
                battle, identities = snapshot(worker, scratch / 'terminal.s01', catalogue)
                write(retained / 'terminal.json', battle)
                write(retained / 'identities.json', identities)
                check_entry(case, rows, battle)
                result['entry_registered'] = True
                result.update(check_actors(case, battle, identities), actor_identity_verified=True)
            except Exception as error:
                result['error'] = str(error)
                if worker is not None and worker.paused and worker.process.poll() is None:
                    try:
                        terminal = scratch / 'failure.s01'
                        battle, identities = snapshot(worker, terminal, catalogue)
                        write(retained / 'failure-terminal.json', battle)
                        write(retained / 'failure-identities.json', identities)
                        with gzip.open(retained / 'failure.s01.gz', 'wb') as stream:
                            stream.write(terminal.read_bytes())
                    except Exception as error:
                        result['snapshot_error'] = str(error)
            finally:
                for path in [*scratch.glob('observations/*.json'), *scratch.glob('warmup/*.json')]:
                    with gzip.open(retained / (path.name + '.gz'), 'wb') as stream:
                        stream.write(path.read_bytes())
                result['seconds'] = time.monotonic() - start
                write(retained / 'result.json', result)
                results.append(result)
                print(json.dumps(result), flush=True)
                shutil.rmtree(scratch)
                if worker is not None and 'error' in result:
                    worker.close()
                    worker = None
        if worker is not None:
            worker.close(complete=True)
            worker = None
    finally:
        if worker is not None:
            worker.close()
        for path in work.glob('worker-*/session.json'):
            shutil.copyfile(path, output / (path.parent.name + '-session.json'))
        executed = len(results) == len(selected['cases'])
        write(output / 'results.json', dict(results=results, reused_entries=selected['reused_entries'],
            execution_complete=executed,
            entry_complete=executed and all(r['entry_registered'] for r in results),
            actor_identity_complete=executed and all(r['actor_identity_verified']
                                    for r in results + selected['reused_entries'])))
        tempfile.tempdir = previous_tmpdir
        alias.cleanup()
        shutil.rmtree(work)
    return int(any('error' in result for result in results))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['plan', 'run'])
    parser.add_argument('--case', type=Path, default=Path('tools/oracle/cases/battle-formations.json'))
    parser.add_argument('--assets', type=Path, default=Path('local/all-assets'))
    selected = parser.add_mutually_exclusive_group(required=True)
    selected.add_argument('--ids', type=int, nargs='+')
    selected.add_argument('--range', type=int, nargs=2, metavar=('FIRST', 'LAST'), help='inclusive bounds')
    parser.add_argument('--missing-entry', action='store_true', help='reuse retained entry receipts; identity remains separate')
    parser.add_argument('--arena', type=int, help='explicit component arena override')
    parser.add_argument('--disc', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    os.chdir(REPO)
    profile = json.loads(args.case.read_text())
    chosen = selection(args, profile)
    if args.command == 'plan':
        print(json.dumps(chosen, indent=2))
        return 0
    if args.disc is None or args.output is None:
        parser.error('run requires --disc and a fresh --output')
    return run(args, profile, chosen)


if __name__ == '__main__':
    raise SystemExit(main())
