#!/usr/bin/env python3
"""Compare scenario-driven effect lifecycles against persistent Dolphin workers."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import copy
import html
import fnmatch
import hashlib
import json
import math
from pathlib import Path
import sys
import shutil
import time

import numpy as np
from PIL import Image

from effect_workers import DOLPHIN_BACKEND, DOLPHIN_OPTIONS, DolphinFixture, NativeFixture, renderer_environment, set_movie_backend
from effect_compare import GATES, compare, plane, run_command, validate_matte, validate_checker, write
from effect_cases import cases, case_names, random_cases, scenario_commands, minimize, capture_settings, SYNC_WHITE
from effect_fixture import prepare, command_words, interaction_words, EFFECT_START, INTERACTION_START, SCENE_SETUP
from state import State, digest, inspect


PRESENTATION_FLUSH = 32
PROFILES = tuple(Path(__file__).with_name('cases') / f'effect-{name}.json' for name in (
    'bases', 'tower', 'rings', 'ring-bomb', 'ring-bubble', 'ring-shrink', 'ring-sunlight', 'stations'))


def program(commands, interactions=()):
    words = [word for op, args in commands for word in command_words(op, args)]
    words += [0x20ff]
    if not interactions:
        return [5, 0, 0, 0, 0] + words
    registry = [word for actor in interactions
                for value in (0, actor, len(words)) for word in (value >> 16, value & 65535)]
    return [4+len(registry), 0, 0, len(interactions)] + registry + words + interaction_words()


def presentation_delay(directory, maximum=PRESENTATION_FLUSH):
    # Register only against the clock marker, which must match exactly.
    pixels = []
    for frame in range(30 + maximum):
        with Image.open(directory/f'frame-{frame:04}.png') as image:
            pixel = image.convert('RGB').getpixel((10, 10))
        pixels.append(pixel)
    offsets = [offset for offset in range(maximum+1)
               if all(len(set(pixels[frame+offset])) == 1
                      and (pixels[frame+offset][0] > 0) == (frame in SYNC_WHITE)
                      for frame in range(3, 29))]
    if len(offsets) != 1:
        raise ValueError('missing or ambiguous input-clock marker')
    return offsets[0]


def compare_frame(reference, actual, background=None, actual_background=None):
    roi = (0, 0, 640, 400)  # Field action prompt is outside the effect stage.
    background = background or Image.new('RGB', reference.size)
    return compare(reference, actual, background, roi, actual_background)


def completed_capture(directory):
    try:
        return json.loads((directory/'capture.json').read_text()).get('complete', False)
    except (FileNotFoundError, ValueError):
        return False


def capture_identity(inputs):
    """Checkpoint content identifies a capture; its local filename does not."""
    return dict(inputs, source=inputs['source']['sha256'])


def validate_capture(capture, fixture, movie_hash, frames):
    """Reuse depends on actual simulator inputs, independent of comparison code."""
    sample, = capture['samples']
    command = capture['command']
    if (not capture['complete'] or not capture['presentation']['one_image_per_vi']
            or sample['state_sha256'] != fixture['fixture_sha256']
            or capture['movie_sha256'] != movie_hash
            or capture['renderer']['backend'] != DOLPHIN_BACKEND
            or capture['renderer']['environment'] != renderer_environment()
            or command[command.index('-v'):] != DOLPHIN_OPTIONS
            or sample['frames'] < frames
            or set(sample['images']) != {f'frame-{i:04}.png' for i in range(sample['frames'])}):
        raise ValueError('reference capture inputs differ or recording is incomplete')
    return sample


def failure_signature(case, result):
    """Shrink a behavior failure without substituting an oracle/setup error."""
    if not result['source_complete'] or result.get('fixture_error'):
        return None
    if 'error' in result:
        if result['phase'] == 'native' and not result.get('timeout'):
            return ('native', result['error'].rsplit(': ', 1)[-1])
        return None
    for kind, failed in [('expiry', not result['expired']),
                         ('containment', not result['contained']),
                         ('pixels', any(not frame['passed'] for frame in result['frames']))]:
        if failed:
            return kind
    return None


def assess(case, frames):
    """Validate source expectations before attributing a difference to Resonance."""
    def expired(engine):
        return not case.get('expires') or all(row[engine+'_active_pixels'] == 0 for row in frames[-5:])
    def contained(engine):
        return not case.get('contained') or all(
            row[engine+'_bounds'] is None or
            (0 < row[engine+'_bounds'][0] <= row[engine+'_bounds'][2] < 639 and
             0 < row[engine+'_bounds'][1] <= row[engine+'_bounds'][3] < 399) for row in frames)
    visible = any(row['reference_active_pixels'] >= GATES['minimum_active_pixels'] for row in frames)
    invalid = ('source visibility differs from the fixture' if visible != case.get('visible', True) else
               'source effect has not expired; extend the fixture' if not expired('reference') else
               'source effect leaves the fixture bounds' if not contained('reference') else None)
    return dict(visible=visible, expired=expired('actual'), contained=contained('actual'), fixture_error=invalid,
                passed=invalid is None and expired('actual') and contained('actual') and all(r['passed'] for r in frames))


def checkerboard(camera, depth):
    # Leave the clock marker and action prompt outside the textured stage.
    for y in range(4):
        for x in range(7):
            position, units = plane(camera, 80+x*80, 80+y*80, depth)
            shade = 64 if (x+y) % 2 else 16
            # Whole even dimensions cover tile boundaries after vertex quantization.
            yield {'position': position, 'size': [2*math.ceil(40*units)]*2, 'shade': shade}


def controller_inputs(inputs):
    """Both controller polls in an update share one held state."""
    buttons = {'a': 'accept', 'b': 'cancel', 'x': 'ring', 'y': 'menu',
               'z': 'skit', 'start': 'start', 'l': 'previous_page', 'r': 'next_page'}
    if any(row['poll'] % 2 or row['duration'] % 2 for row in inputs):
        raise ValueError('effect controller input must span whole updates')
    ticks = sorted({poll // 2 for row in inputs for poll in (row['poll'], row['poll']+row['duration'])})
    return [{'update': tick, 'buttons': sorted({buttons[button] for row in inputs
             if row['poll'] <= tick*2 < row['poll']+row['duration'] for button in row['buttons']})}
            for tick in ticks]


class ControllerMovies:
    """Record controller timelines as requested, including reduced candidates."""
    def __init__(self, source, start_poll, output):
        self.source, self.start_poll, self.output = source, start_poll, output
        self.recordings = {}
        output.mkdir(exist_ok=True)

    def get(self, inputs):
        key = json.dumps(inputs, sort_keys=True)
        if key not in self.recordings:
            name = hashlib.sha256(key.encode()).hexdigest()[:16]
            config, movie = self.output/(name+'.json'), self.output/(name+'.dtm')
            write(config, {'game_id': 'GQSEAF', 'polls': 10000, 'rtc': 1700000000, 'inputs': inputs})
            run_command(['target/debug/resonance-oracle', 'dtm', config, '--prefix', str(self.source)+'.dtm',
                         '--start-poll', self.start_poll, '--output', movie], self.output/(name+'.log'))
            set_movie_backend(movie)
            self.recordings[key] = (movie, digest(movie))
        return self.recordings[key]


def run_profile(args):
    profile = json.loads(args.case.read_text())
    source = Path(profile['source']['path'])
    template = Path(profile['native_sequence']['path'])
    for key, path in [('source', source), ('native_sequence', template)]:
        if digest(path) != profile[key]['sha256']:
            raise ValueError(f'{key} fixture hash differs')
    if args.output.exists() and not args.resume:
        raise ValueError('output must be a fresh directory')
    if args.resume and args.reference:
        raise ValueError('resume uses captures in the output directory')
    started = time.monotonic()
    native_digest = digest(args.native)
    base = State(source)
    observed = inspect(base)
    sequence = json.loads(template.read_text())
    camera = sequence['isolation']['camera']
    if any(not np.allclose(camera[k], observed['field_camera'][k], atol=.001, rtol=0)
           for k in ['position', 'target']):
        raise ValueError('source/native cameras are not registered')
    recorded = args.output if args.resume and (args.output/'inputs.json').is_file() else args.reference
    if recorded:
        if args.random_cases:
            raise ValueError('cached runs replay their recorded cases; omit random-cases')
        selected = json.loads((recorded/'inputs.json').read_text())['cases']
    else:
        catalogue = profile_cases(profile, camera)
        if not catalogue:
            raise ValueError('profile has no eligible effects')
        selected = catalogue + list(random_cases(camera, catalogue, args.random_cases, args.seed))
    selected = [case for case in selected
                if args.only is None or fnmatch.fnmatchcase(case['name'], args.only)]
    if args.replay_case:
        selected = [json.loads(args.replay_case.read_text())]
    if not selected:
        return None
    args.output.mkdir(parents=True, exist_ok=args.resume)
    commands = SCENE_SETUP
    matte = {'position': plane(camera, 320, 240, profile.get('matte_depth', 20))[0], 'rotation': [0, 0, 0],
             'size': [100000, 100000], 'rgba': [0, 0, 0, 255], 'world_space': False,
             'blend': 0, 'uv_bytes': [30, 25, 0, 0], 'texture': 2}
    def input_key(case):
        return json.dumps(case.get('inputs', profile.get('inputs', [])), sort_keys=True)
    executable = Path(shutil.which('dolphin-emu')).resolve()
    watch_locations = {f'{base.read(0x8035a578)+EFFECT_START:x}': 'effect_start'}
    if any(case.get('interactions') for case in selected):
        watch_locations[f'{base.read(0x8035a578)+INTERACTION_START:x}'] = 'interaction_started'
    oracle_files = [executable, executable.with_name('.'+executable.name+'-wrapped')]
    oracle_files += [p for d in ['config', 'game-settings'] for p in Path(__file__).with_name(d).glob('*')]
    fingerprint = {'presentation_flush': PRESENTATION_FLUSH, 'commands': commands, 'seed': 1,
                   'renderer': {'backend': DOLPHIN_BACKEND, 'environment': renderer_environment()},
                   'matte': matte, 'watch_locations': watch_locations,
                   'dolphin_profile': {str(p): digest(p) for p in oracle_files if p.is_file()}, 'source': profile['source'], 'source_movie': digest(str(source)+'.dtm'),
                   'disc': digest(args.disc)}
    plan = {'inputs': fingerprint, 'cases': selected}
    if args.resume and recorded:
        previous = json.loads((args.output/'inputs.json').read_text())
        if previous['cases'] != selected or capture_identity(previous['inputs']) != capture_identity(fingerprint):
            raise ValueError('resume inputs differ')
    else:
        write(args.output/'inputs.json', plan)
    if args.reference:
        recorded = json.loads((args.reference/'inputs.json').read_text())['inputs']
        if capture_identity(recorded) != capture_identity(fingerprint):
            raise ValueError('cached reference provenance differs')
    def prepare_case(case, out):
        if Path(case['name']).name != case['name'] or case['name'] in ['.', '..']:
            raise ValueError('invalid case name')
        out.mkdir(exist_ok=args.resume)
        write(out/'case.json', case)
        case = capture_settings(case)
        native = copy.deepcopy(sequence)
        scene = native['scene']['arrival']
        scene['checkpoint']['position'] = observed['controlled_actor']['position']
        scene['checkpoint']['heading'] = observed['controlled_actor']['heading_current']
        if 'map' in case:
            scene['checkpoint']['map_id'] = case['map']
        native.update(at={'kind': 'tick', 'update': 0}, updates=case['updates'], capture_frames=[],
                      inputs=controller_inputs(json.loads(input_key(case))))
        scene['script'] = program(commands+[[0x64, [0, 1]]]+list(scenario_commands(case)), case.get('interactions', ()))
        scene['checkpoint']['progress']['random_state'] = case.get('seed', 1)
        backdrop = list(checkerboard(case.get('camera', camera), case['backdrop_depth'])) if case.get('checkerboard') else []
        native['isolation'] = dict(background=[0, 0, 0], remove_actors=[], clear_effects=True,
                                   cancel_scripts=False, visible_actors=[],
                                   camera=case.get('camera', camera), backdrop=backdrop)
        write(out/'native.json', native)
        case_matte = dict(matte, position=plane(case.get('camera', camera), 320, 240,
                          max(profile.get('matte_depth', 20), case['backdrop_depth']+2))[0])
        fixture = prepare(base, out/'fixture.s01', observed, [dict(tile, rotation=[0, 0, 0], rgba=[tile['shade']]*3+[255],
                          world_space=False, blend=0, uv_bytes=[32, 32, 0, 0], texture=2) for tile in backdrop],
                          commands=commands+list(scenario_commands(case)), seed=case.get('seed', 1),
                          matte=None if case.get('no_matte') else case_matte,
                          interactions=case.get('interactions', ()))
        write(out/'fixture.json', fixture)
    def worker(index, shard):
        dolphin = native_worker = None
        movies = ControllerMovies(source, observed['movie']['input_count'], args.output/f'inputs-{index}')
        results = []
        def execute(case, out, reference=None):
            nonlocal dolphin, native_worker
            case = capture_settings(case)
            name = case['name']
            source_complete = False
            phase = 'source'
            try:
                movie, movie_hash = movies.get(json.loads(input_key(case)))
                fixture = json.loads((out/'fixture.json').read_text())
                if not reference and not completed_capture(out/'dolphin'):
                    if (out/'dolphin').exists():
                        shutil.rmtree(out/'dolphin')
                    if dolphin and dolphin.metadata['movie_sha256'] != movie_hash:
                        dolphin.close(complete=True)
                        dolphin = None
                    if dolphin is None:
                        dolphin = DolphinFixture(args.disc, movie, out/'fixture.s01',
                            args.output/f'worker-{index}-{time.time_ns()}', fixture['marker_address'], watch_locations,
                            disc_sha256=fingerprint['disc'])
                    dolphin.sequence(out/'fixture.s01', out/'dolphin', fixture['case_marker'], case['updates']+PRESENTATION_FLUSH)
                ref = reference/name/'dolphin' if reference else out/'dolphin'
                capture = json.loads((ref/'capture.json').read_text())
                sample = validate_capture(capture, fixture, movie_hash, case['updates'])
                if digest(ref/'observations.json') != sample['observations_sha256']:
                    raise ValueError('reference observations changed')
                observations = json.loads((ref/'observations.json').read_text())
                if case.get('interactions') and not any(row['interaction_started'] == 1 for row in observations):
                    raise ValueError('source interaction did not run')
                for file, expected in sample['images'].items():
                    if digest(ref/file) != expected:
                        raise ValueError('reference image changed')
                source_complete = True
                delay = presentation_delay(ref)
                if sample['frames'] < case['updates'] + delay:
                    raise ValueError('reference capture ends before the registered comparison')
                started_at = next(i for i, row in enumerate(observations) if row['effect_start'] == 1)
                if started_at == 0:
                    raise ValueError('effect clock marker was already set')
                seed = observations[started_at-1]['random_state']
                native = json.loads((out/'native.json').read_text())
                native['random_seed'] = {'update': case['warmup'], 'state': seed,
                                        'effect_tick': observations[started_at]['presentation_counter']-1}
                write(out/'native.json', native)
                if (out/'native').exists():
                    shutil.rmtree(out/'native')
                phase = 'native'
                if native_worker is None:
                    native_worker = NativeFixture(args.native, args.cooked, args.output/f'native-worker-{index}.log')
                native_worker.capture(out/'native.json', out/'native')
                phase = 'comparison'
                if digest(args.native) != native_digest:
                    raise ValueError('native executable changed during comparison')
                if presentation_delay(out/'native', maximum=0) != 0:
                    raise ValueError('native input-clock marker drifted')
                for frame in range(3, case['warmup']):
                    if frame not in SYNC_WHITE:
                        validate_matte(Image.open(ref/f'frame-{frame+delay:04}.png'), [0, 0, 0])
                        validate_matte(Image.open(out/'native'/f'frame-{frame:04}.png'), [0, 0, 0])
                frames = []
                background = actual_background = None
                if case.get('checkerboard'):
                    background = Image.open(ref/f'frame-{case["warmup"]-1+delay:04}.png')
                    actual_background = Image.open(out/'native'/f'frame-{case["warmup"]-1:04}.png')
                    validate_checker(background)
                    validate_checker(actual_background)
                    if not compare_frame(background, actual_background)['passed']:
                        raise ValueError('checkerboard baseline differs before the effect')
                for frame in range(case['warmup'], case['updates']):
                    file = f'frame-{frame+delay:04}.png'
                    reference, actual = Image.open(ref/file), Image.open(out/'native'/f'frame-{frame:04}.png')
                    frames.append({'frame': frame, **compare_frame(reference, actual, background, actual_background)})
                result = {'case': name, 'presentation_delay': delay, 'frames': frames,
                          'effect_seed': seed, 'source_complete': True, **assess(case, frames)}
                result['failure_images'] = []
                for row in [row for row in frames if not row['passed']][:8]:
                    frame = row['frame']
                    reference = Image.open(ref/f'frame-{frame+delay:04}.png').convert('RGB')
                    actual = Image.open(out/'native'/f'frame-{frame:04}.png').convert('RGB')
                    pair = Image.new('RGB', (1280, 480))
                    pair.paste(reference); pair.paste(actual, (640, 0))
                    file = f'failure-{frame:04}.png'
                    pair.save(out/file)
                    result['failure_images'].append(file)
                if result['passed']:
                    for file in (out/'native').glob('frame-*.png'):
                        file.unlink()
                write(out/'results.json', result)
                print(name, sum(row['passed'] for row in frames), '/', len(frames), result['fixture_error'] or '', flush=True)
                return result
            except Exception as error:
                result = {'case': name, 'frames': [], 'passed': False,
                          'source_complete': source_complete, 'error': str(error),
                          'phase': phase, 'timeout': isinstance(error, TimeoutError)}
                write(out/'results.json', result)
                print(name, result['error'], flush=True)
                if native_worker:
                    native_worker.close()
                    native_worker = None
                if dolphin:
                    dolphin.close()
                    dolphin = None
                return result
            finally:
                # Keep inputs and hashes; discard the temporary savestate.
                for suffix in ('fixture.s01', 'fixture.s01.dtm'):
                    (out/suffix).unlink(missing_ok=True)
        try:
            # Batch equal controller recordings so each Dolphin stays resident.
            for case in sorted(shard, key=input_key):
                out = args.output/case['name']
                prepare_case(case, out)
                result = execute(case, out, args.reference)
                results.append(result)
                failure = failure_signature(case, result)
                if args.minimize and failure is not None:
                    attempts = out/'minimize'
                    if attempts.exists():
                        shutil.rmtree(attempts)
                    attempts.mkdir()
                    trial = 0
                    def fails(candidate):
                        nonlocal trial
                        trial += 1
                        output = attempts/str(trial)
                        prepare_case(candidate, output)
                        measured = execute(candidate, output)
                        retained = failure_signature(candidate, measured) == failure
                        if retained:
                            best = attempts/'retained'
                            shutil.rmtree(best, ignore_errors=True)
                            output.rename(best)
                        else:
                            shutil.rmtree(output)
                        return retained
                    write(out/'minimal.json', minimize(case, fails))
            if dolphin:
                dolphin.close(complete=True)
                dolphin = None
            return results
        finally:
            if native_worker:
                native_worker.close()
            if dolphin:
                dolphin.close()
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(worker, index, selected[index::args.workers])
                   for index in range(min(args.workers, len(selected)))]
        results = [result for future in futures for result in future.result()]
    results.sort(key=lambda r: r['case'])
    write(args.output/'results.json', {'results': results, 'inputs': fingerprint, 'gates': GATES,
        'source_complete': all(r['source_complete'] for r in results), 'native_sha256': native_digest, 'workers': args.workers,
        'seconds': time.monotonic()-started, 'passed': all(r['passed'] for r in results)})
    rows = ''.join(f'<tr><td>{html.escape(r["case"])}</td><td>{sum(f["passed"] for f in r["frames"])}/{len(r["frames"])}</td><td>{r["passed"]}</td><td>' +
                   ' '.join(f'<a href="{r["case"]}/{file}">{file}</a>' for file in r.get('failure_images', [])) +
                   html.escape(r.get('error') or r.get('fixture_error') or '')+'</td></tr>' for r in results)
    (args.output/'report.html').write_text('<!doctype html><meta charset="utf-8"><title>Effect lifecycles</title>'
        '<h1>Effect lifecycles</h1><p>Independent scenario commands, fixed cameras and consecutive updates. '
        'No image alignment or brightness fitting.</p><table><tr><th>Case</th><th>Frames</th><th>Pass</th></tr>'+rows+'</table>')
    return 0 if all(r['passed'] for r in results) else 1


def profile_cases(profile, camera):
    return [dict(case, inputs=case.get('inputs', profile.get('inputs', []))) for case in cases(camera, profile.get('family', 'field'))
            if eligible(profile, case['name'])]


def eligible(profile, name):
    return (fnmatch.fnmatchcase(name, profile.get('only', '*'))
            and not any(fnmatch.fnmatchcase(name, pattern) for pattern in profile.get('exclude', [])))


def run(args):
    if not 1 <= args.workers <= 16 or not 0 <= args.random_cases <= 1000:
        raise ValueError('workers must be 1–16 and random-cases must be 0–1000')
    if args.case:
        result = run_profile(args)
        if result is None:
            raise ValueError('no matching cases')
        return result
    if args.replay_case:
        raise ValueError('replay-case requires the source stage selected with --case')
    if args.resume and args.reference:
        raise ValueError('resume uses captures in the output directory')
    if (args.resume or args.reference) and args.random_cases:
        raise ValueError('cached runs replay their recorded cases; omit random-cases')
    recorded = args.reference or (args.output if args.resume else None)
    if recorded:
        plan = json.loads((recorded/'plan.json').read_text())
        if args.reference:
            report = json.loads((recorded/'results.json').read_text())
            if not report['complete']:
                raise ValueError('reference suite is incomplete; resume it first')
            completed = {p['profile'] for p in report['profiles']}
            plan = [stage for stage in plan if Path(stage['profile']).stem in completed]
            if args.only is not None:
                for stage in plan:
                    stage['only'] = args.only
        elif args.only is not None and any(stage['only'] != args.only for stage in plan):
            raise ValueError('resume must use the recorded case selection')
    else:
        profiles = [json.loads(path.read_text()) for path in PROFILES]
        weights = [sum(eligible(profile, name) for name in case_names(profile.get('family', 'field'))) for profile in profiles]
        total, before, plan = sum(weights), 0, []
        for path, weight in zip(PROFILES, weights):
            count = args.random_cases*(before+weight)//total - args.random_cases*before//total
            plan.append(dict(profile=path.name, seed=args.seed, only=args.only, random_cases=count))
            before += weight
    args.output.mkdir(parents=True, exist_ok=args.resume)
    write(args.output/'plan.json', plan)
    profiles = []
    paths = {path.name: path for path in PROFILES}
    for planned in plan:
        path = paths[planned['profile']]
        stage = copy.copy(args)
        stage.case = path
        stage.output = args.output/path.stem
        stage.reference = args.reference/path.stem if args.reference else None
        stage.resume = args.resume and stage.output.exists()
        stage.seed = planned['seed']
        stage.only = args.only if args.reference and args.only is not None else planned['only']
        stage.random_cases = (0 if stage.reference or (stage.resume and (stage.output/'inputs.json').is_file())
                              else planned['random_cases'])
        if run_profile(stage) is None:
            continue
        report = json.loads((stage.output/'results.json').read_text())
        profiles.append({'profile': path.stem, 'cases': len(report['results']),
                         'passed': report['passed'], 'source_complete': report['source_complete']})
        write(args.output/'results.json', {'profiles': profiles, 'complete': False, 'passed': False})
    if not profiles:
        raise ValueError('no matching cases')
    passed = all(p['passed'] for p in profiles)
    write(args.output/'results.json', {'profiles': profiles, 'complete': True, 'passed': passed})
    return 0 if passed else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case', type=Path, help='Run one stage; defaults to all effect stages')
    parser.add_argument('--disc', type=Path, required=True)
    parser.add_argument('--cooked', type=Path, default=Path('local/all-assets'))
    parser.add_argument('--native', type=Path, default=Path('target/debug/examples/effect_sequence'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--resume', action='store_true', help='Reuse completed Dolphin captures from an interrupted run')
    parser.add_argument('--workers', type=int, default=2)
    parser.add_argument('--only')
    parser.add_argument('--replay-case', type=Path, help='Replay a saved case.json or minimal.json exactly')
    parser.add_argument('--minimize', action='store_true', help='Reduce runtime, lifecycle and visual failures')
    parser.add_argument('--random-cases', type=int, default=0)
    parser.add_argument('--seed', type=int, default=20261006)
    sys.exit(run(parser.parse_args()))
