#!/usr/bin/env python3
"""Capture named arte images in original GC Dolphin from a newly pinned checkpoint.

Uses existing data-only fixture preparation and bounded capture workers. The
current catalogue selects work; historical recipes supply input setup only.
Their exclusions never remove a current binding or establish visual acceptance.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import shutil
import sys

ORACLE = Path(__file__).resolve().parents[1] / 'oracle'
sys.path.insert(0, str(ORACLE))
import battle_artes
from state import digest
from source_worker import VisualWorker, capture, ready_state, write_context


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def recipe(row, recipes):
    binding = row['binding']
    found = recipes.get((binding.get('character'), binding.get('arte')))
    if row['category'] != 'party' or found is None:
        return None, 'No original-game input fixture established for this current binding.'
    if found.get('pending_setup'):
        return None, 'Requires explicit source setup: ' + found['pending_setup']
    result = copy.deepcopy(found)
    result['name'] = row['display_name']
    return result, None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalogue', type=Path, required=True)
    parser.add_argument('--reference', type=Path, required=True,
                        help='Fresh reference directory containing provenance.json, boot-field.s01 and companion DTM')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--character', type=int, action='append', default=[])
    parser.add_argument('--case', action='append', default=[], help='Exact current catalogue case ID')
    parser.add_argument('--limit', type=int, help='Bound the selected cases for a throughput pilot')
    args = parser.parse_args()
    catalogue = read(args.catalogue)
    reference = args.reference.resolve()
    provenance = read(reference / 'provenance.json')
    tree = reference / provenance['extracted_tree_manifest']
    if digest(tree) != provenance['extracted_tree_sha256']:
        raise ValueError('extracted source-tree manifest changed')
    state = reference / 'boot-field.s01'
    for path, key in [(state, 'boot_state_sha256'), (Path(str(state) + '.dtm'), 'boot_state_movie_sha256')]:
        if digest(path) != provenance[key]:
            raise ValueError('fresh reference input changed: ' + str(path))
    disc = Path(provenance['source_executable'])
    if digest(disc) != provenance['source_dol_sha256']:
        raise ValueError('original GameCube executable changed')
    library = Path(catalogue['libraries']['gamecube'])
    techniques = library / 'game/techniques.json'
    legacy = read(ORACLE / 'cases/battle-artes.json')
    recipes = {(row['character'], row['arte']): row for row in legacy['cases']}
    selected = [row for row in catalogue['cases'] if row['edition'] == 'gamecube'
                and (not args.character or row['binding'].get('character') in args.character)
                and (not args.case or row['id'] in args.case)]
    if args.limit is not None:
        if args.limit < 1:
            raise ValueError('limit must be positive')
        selected = selected[:args.limit]
    if not selected:
        raise ValueError('no current GameCube cases selected')
    output = args.output.resolve()
    if not output.is_relative_to(Path('local/battle-capture-work').resolve()):
        raise ValueError('source captures belong under local/battle-capture-work')
    output.mkdir(parents=True, exist_ok=False)
    manifest = {key: copy.deepcopy(legacy[key]) for key in
                ('version', 'game_id', 'fixture_defaults', 'pending_setups', 'capture_frames')}
    manifest['inputs'] = {
        'state': {'path': str(state), 'sha256': digest(state)},
        'movie': {'path': str(state) + '.dtm', 'sha256': digest(Path(str(state) + '.dtm'))},
        'techniques': {'path': str(techniques), 'sha256': digest(techniques)},
        'disc_sha256': digest(tree),
        'disc_hash_scope': 'Extracted source-tree manifest, not an ISO hash; see source-provenance.json',
    }
    manifest['cases'] = [found for row in selected if (found := recipe(row, recipes)[0])]
    # Leave ordinary neutral entry enough time to settle before the one future B.
    manifest['fixture_defaults']['press_after_polls'] = 2000
    manifest_path = output / 'source-manifest.json'
    write(manifest_path, manifest)
    write(output / 'source-provenance.json', {
        **provenance, 'catalogue': str(args.catalogue.resolve()),
        'catalogue_sha256': digest(args.catalogue), 'techniques_sha256': digest(techniques),
        'source_tree_manifest': str(tree), 'source_reference': str(reference),
        'scope': 'Original GC Dolphin images, with explicit fixture assistance. No Resonance visual comparison.',
    })
    results = [{'id': row['id'], 'binding': row['binding'], 'display_name': row['display_name'],
                'status': 'pending', 'visual_review': 'pending'} for row in selected]
    report = {'source_kind': 'dolphin', 'simultaneous_dolphin_limit': 1,
              'native_comparison': 'not_performed', 'worker_closed': False, 'cases': results}
    os.environ.pop('LD_LIBRARY_PATH', None)
    os.environ['RESONANCE_DOLPHIN'] = provenance['dolphin']
    worker = None
    marker = None
    try:
        for row, result in zip(selected, results):
            found, reason = recipe(row, recipes)
            if reason:
                result['reason'] = reason
                write(output / 'nativecapture-report.json', report)
                print(json.dumps(result), flush=True)
                continue
            case_output = output / row['id'].replace(':', '-')
            key = f"{found['character']}:{found['arte']}"
            prepare_args = argparse.Namespace(case=key, state=None, techniques=None,
                                              output=case_output, manifest=manifest_path)
            try:
                battle_artes.prepare_case(prepare_args, manifest)
            except Exception as error:
                result.update(reason='Source preparation failed: ' + str(error))
                write(output / 'nativecapture-report.json', report)
                print(json.dumps(result), flush=True)
                continue
            record = read(case_output / 'fixture.json')
            if worker is None:
                marker = record['fixture']['marker_address']
                worker = VisualWorker(str(disc), case_output / 'input.dtm', case_output / 'entry.s01',
                                       output / 'worker', marker, read(case_output / 'locations.json'),
                                       disc_sha256=digest(tree))
                report['pid'] = worker.process.pid
                print(json.dumps({'worker_pid': worker.process.pid, 'display': worker.env['DISPLAY']}), flush=True)
            elif marker != record['fixture']['marker_address']:
                raise ValueError('current fixture changed the shared marker address')
            try:
                ready = ready_state(worker, case_output, record)
                captured = capture(worker, case_output, record, ready)
                write_context(case_output, record, library)
                result.update(status='captured', admitted=captured['admitted'],
                              action_start_clock=captured['admission_clock'],
                              age_origin='First requested native identity near the source payment',
                              capture_complete=True, path=str(case_output.relative_to(output)),
                              fixture=str((case_output / 'visual-fixture.json').relative_to(output)),
                              images=[{'age': sample['age'], 'phase': sample['phase'],
                                       'path': str(Path(sample['path']).relative_to(output)),
                                       'sha256': sample['sha256']} for sample in captured['samples']],
                              source_provenance='source-provenance.json')
                # Entry and ready states reproduce the case. Intermediate
                # snapshots are large and no longer needed after capture.
                for name in ('initialized.s01', 'neutral-entry.s01', 'warm.s01'):
                    for suffix in ('', '.dtm'):
                        (case_output / (name + suffix)).unlink(missing_ok=True)
            except Exception as error:
                if 'milestone exceeded its guest-frame budget' in str(error):
                    result.update(reason='Source phase setup remains unsupported: ' + str(error))
                    write(output / 'nativecapture-report.json', report)
                    print(json.dumps(result), flush=True)
                    continue
                result.update(status='capture_failed', reason=str(error))
                report['stopped_after_capture_failure'] = row['id']
                write(output / 'nativecapture-report.json', report)
                print(json.dumps(result), flush=True)
                break
            write(output / 'nativecapture-report.json', report)
            print(json.dumps({key: result[key] for key in ('id', 'status', 'path')}), flush=True)
    finally:
        if worker:
            # Preserve failed checkpoint evidence before the existing worker's cleanup.
            if report.get('stopped_after_capture_failure'):
                shutil.copytree(worker.user / 'StateSaves', output / 'failed-state-files', dirs_exist_ok=True)
            worker.close(complete=not report.get('stopped_after_capture_failure'))
        report['worker_closed'] = True
        report['captured'] = sum(row['status'] == 'captured' for row in results)
        report['pending'] = sum(row['status'] == 'pending' for row in results)
        write(output / 'nativecapture-report.json', report)


if __name__ == '__main__':
    main()
