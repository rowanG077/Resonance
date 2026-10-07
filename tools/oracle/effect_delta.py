#!/usr/bin/env python3
"""Paired low-level field-effect tests. No ability scripts or random composites."""
import argparse
import copy
import html
import json
import math
from pathlib import Path
import subprocess
import sys

import numpy as np
from PIL import Image

from effect_fixture import digest, prepare
from state import inspect

# Gates are shared across cases; known differences stay failures, never tuned away.
GATES = {'pixel_tolerance': 8, 'max_changed_fraction': 0.05,
         'max_mean_error': 3.0, 'max_bounds_delta': 1, 'minimum_active_pixels': 16}
FRAME = 10
FIELD_OF_VIEW = 27
TELEPORTER_RING = 68610


def write(path, data):
    Path(path).write_text(json.dumps(data, indent=2, allow_nan=False) + '\n')


def compare(reference, actual, background, roi, actual_background=None):
    r = np.asarray(reference.convert('RGB'), dtype=np.int16)
    a = np.asarray(actual.convert('RGB'), dtype=np.int16)
    b = np.asarray(background.convert('RGB'), dtype=np.int16)
    ab = np.asarray((actual_background or background).convert('RGB'), dtype=np.int16)
    if r.shape != a.shape or r.shape != b.shape or r.shape != ab.shape:
        raise ValueError('capture dimensions differ')
    x, y, w, h = roi
    if min(x, y) < 0 or min(w, h) <= 0 or x+w > r.shape[1] or y+h > r.shape[0]:
        raise ValueError('comparison ROI is outside the capture')
    r, a, b, ab = [v[y:y+h, x:x+w] for v in [r, a, b, ab]]
    rm, am = abs(r-b).max(2) > 2, abs(a-ab).max(2) > 2
    mask = rm | am
    def bounds(m):
        ys, xs = np.where(m)
        return [int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max())] if xs.size else None
    rb, ab = bounds(rm), bounds(am)
    error = abs(a-r)
    count = int(mask.sum())
    result = {'reference_active_pixels': int(rm.sum()), 'actual_active_pixels': int(am.sum()),
              'reference_bounds': rb, 'actual_bounds': ab,
              'mean_error': float(error[mask].mean()) if count else None,
              'changed_fraction': float((error.max(2)[mask] > GATES['pixel_tolerance']).mean()) if count else None,
              'bounds_delta': max(abs(v-w) for v, w in zip(rb, ab)) if rb and ab else None}
    result['passed'] = (min(result['reference_active_pixels'], result['actual_active_pixels']) >= GATES['minimum_active_pixels']
                        and result['mean_error'] <= GATES['max_mean_error']
                        and result['changed_fraction'] <= GATES['max_changed_fraction']
                        and result['bounds_delta'] <= GATES['max_bounds_delta'])
    return result


def validate_matte(image, color):
    pixels = np.asarray(image.convert('RGB'), dtype=np.int16)
    # Outside every probe and the bottom-right field prompt.
    samples = pixels[10:20, 10:630]
    if np.abs(samples-np.asarray(color)).max() > 1:
        raise ValueError('invalid fixture: the isolation matte is missing or contaminated')


def validate_checker(image):
    samples = np.asarray(image.convert('RGB'), dtype=np.int16)[40, 40::80]
    if (np.abs(samples[0]-samples[1]).max() < 100
            or np.abs(samples[::2]-samples[0]).max() > 3
            or np.abs(samples[1::2]-samples[1]).max() > 3):
        raise ValueError('invalid fixture: the refraction checkerboard is missing or contaminated')


def plane(camera, x, y, depth=1):
    eye, target = [np.asarray(camera[k], dtype=float) for k in ['position', 'target']]
    forward = target-eye
    distance = np.linalg.norm(forward)
    forward /= distance
    right = np.cross(forward, [0, 0, 1]); right /= np.linalg.norm(right)
    up = np.cross(right, forward)
    units = distance * depth * math.tan(math.radians(FIELD_OF_VIEW/2)) / 240
    return (eye + forward*distance*depth + right*(x-320)*units - up*(y-240)*units).tolist(), units


def sprite(recipe, camera, x, y, size, rgba, blend, rotation=(0, 0, 0), world=False, age=0):
    position, units = plane(camera, x, y)
    return {'position': position, 'rotation': list(rotation), 'size': [v*units for v in size],
            'rgba': rgba, 'world_space': world, 'blend': blend, 'age': age,
            'shape': {'kind': 'sprite', 'recipe': recipe, 'uv': None}}


def source_sprite(probe, catalogue):
    shape = probe['shape']
    if shape['kind'] == 'refraction':
        entry = catalogue['air_refraction'] if shape['air'] else catalogue['refraction']['sprite']
    else:
        entry = catalogue['particles'][str(shape['recipe'])] if shape['kind'] == 'leaf' else catalogue['sprites'][str(shape['recipe'])]
    uv = shape.get('uv')
    if not uv:
        uv = entry['uv']
        frames = entry.get('frames', [])
        age = probe.get('age', 0)+1
        if frames:
            if entry.get('repeat'):
                age %= sum(f['ticks'] for f in frames)
            uv = frames[-1]['uv']
            for frame in frames:
                if age < frame['ticks']:
                    uv = frame['uv']; break
                age -= frame['ticks']
    return {**probe, 'uv_bytes': [round(uv[0]*256), round(uv[1]*256),
                                round((uv[2]-uv[0])*256), round((uv[3]-uv[1])*256)],
            'texture': entry['texture']['effect'], 'refraction': shape['kind'] == 'refraction'}


def cases(camera, catalogue):
    # Distinct renderer behavior, not one case per spell or atlas entry.
    samples = [('alpha', 0, 0, False, (0, 0, 0), 0, False),
               ('additive', 4, 1, False, (0, 0, 0), 0, False),
               ('subtractive', 4, 2, False, (0, 0, 0), 0, False),
               ('camera-rotation', 42, 1, False, (0, 0, 33), 0, False),
               ('world-rotation', 42, 1, True, (25, -15, 33), 0, False),
               ('atlas-crop', 52, 0, False, (0, 0, 0), 0, False)]
    samples += [(f'smoke-age-{age}', 1, 0, False, (0, 0, 0), age, True) for age in [0, 5, 6, 12, 48, 55]]
    samples += [(f'loop-age-{age}', 42, 1, False, (0, 0, 0), age, True) for age in [0, 1, 3, 5]]
    probes, regions = [], []
    for i, (name, recipe, blend, world, rotation, age, animated) in enumerate(samples):
        x, y = (i % 4)*160+80, (i//4)*100+80
        probe = sprite(recipe, camera, x, y, [64, 64], [64, 32, 48, 160], blend, rotation, world, age)
        if not animated:
            probe['shape']['uv'] = catalogue['sprites'][str(recipe)]['uv']
        probes.append(probe); regions.append({'name': name, 'rect': [x-48, y-42, 96, 84]})
    for matte, color in [('green', [0, 231, 0]), ('blue', [0, 0, 231])]:
        yield 'sprites-'+matte, color, probes, regions

    # Flutter uses the sprite shader through a separate world-particle submission path.
    leaves, regions = [], []
    for i, rotation in enumerate([(25, -15, 33), (205, -15, 33)]):
        probe = sprite(25, camera, 220+i*200, 210, [160, 160/3], [64, 32, 48, 200], 0, rotation, True)
        probe['shape'] = {'kind': 'leaf', 'recipe': 25, 'motion': {
            'rotation': list(rotation), 'fall_speed': 0, 'spin': 0, 'heading': 0, 'turn_after': 0}}
        leaves.append(probe)
        regions.append({'name': 'front' if i == 0 else 'back', 'rect': [130+i*200, 120, 180, 180]})
    yield 'leaves', [0, 231, 0], leaves, regions

    for name, blend, rotation, scale in [
            ('alpha', 0, [0, 0, 0], [1, 1, 1]),
            ('additive', 1, [0, 0, 0], [1, 1, 1]),
            ('subtractive', 2, [0, 0, 0], [1, 1, 1]),
            ('backface-scale', 1, [180, 0, 35], [1.2, 0.7, 0.9])]:
        probe = sprite(0, camera, 320, 220, [1, 1], [64, 32, 48, 160], blend, rotation, True)
        probe['shape'] = {'kind': 'model', 'resource': TELEPORTER_RING, 'scale': scale}
        yield 'model-'+name, [0, 231, 0], [probe], [{'name': 'mesh', 'rect': [80, 40, 480, 350]}]

    checker = []
    for y in range(6):
        for x in range(8):
            position, units = plane(camera, x*80+40, y*80+40, 2)
            shade = 64 if (x+y) % 2 else 16
            probe = sprite(4, camera, 0, 0, [80, 80], [shade]*3+[255], 0)
            probe.update(position=position, size=[80*units]*2)
            probe['shape']['uv'] = [185/256, 31/256]*2
            checker.append(probe)
    yield 'refraction-background', [0, 231, 0], checker, [{'name': 'checker', 'rect': [20, 20, 600, 380]}]
    for alpha in [128, 255]:
        probes, regions = copy.deepcopy(checker), []
        for i, air in enumerate([False, True]):
            probe = sprite(0, camera, 200+i*240, 220, [180, 180], [64, 64, 64, alpha], 0)
            probe['shape'] = {'kind': 'refraction', 'air': air}
            probes.append(probe)
            regions.append({'name': 'air' if air else 'ripple', 'rect': [100+i*240, 120, 200, 200]})
        yield 'refraction-'+str(alpha), [0, 231, 0], probes, regions


def run_command(args, log):
    with Path(log).open('w') as f:
        subprocess.run(list(map(str, args)), stdout=f, stderr=subprocess.STDOUT, check=True)


def run(args):
    profile = json.loads(args.case.read_text())
    source, sequence_path = [Path(profile[k]['path']) for k in ['source', 'native_sequence']]
    for key, path in [('source', source), ('native_sequence', sequence_path)]:
        if digest(path) != profile[key]['sha256']:
            raise ValueError(f'{key} fixture hash differs')
    if args.output.exists():
        raise ValueError('output must be a fresh directory')
    args.output.mkdir(parents=True)
    observation = inspect(source)
    sequence = json.loads(sequence_path.read_text())
    camera = sequence['isolation']['camera']
    for key in ['position', 'target']:
        if not np.allclose(camera[key], observation['field_camera'][key], atol=0.001, rtol=0):
            raise ValueError('source and native cameras are not registered')
    catalogue_path = args.cooked/'data/embedded/field-effects.json'
    art = json.loads(catalogue_path.read_text())
    catalogue = {**art['effects'], 'particles': art['particles']}
    selected = list(cases(camera, catalogue))
    if args.only and args.only not in [case[0] for case in selected]:
        raise ValueError('unknown case: '+args.only)
    inputs = args.output/'neutral.json'
    write(inputs, {'game_id': 'GQSEAF', 'polls': 500, 'rtc': 1700000000, 'inputs': []})
    results = []
    disc_hash = digest(args.disc)
    for name, color, probes, regions in selected:
        if args.only and name != args.only and not (args.only.startswith('refraction-') and name == 'refraction-background'):
            continue
        out = args.output/name; out.mkdir()
        native = copy.deepcopy(sequence)
        native.update(capture_frames=[FRAME], updates=FRAME+2)
        native['isolation'].update(background=color, effects=probes, clear_effects=True, cancel_scripts=True)
        spec = out/'native.json'; write(spec, native)
        fingerprint = {'profile': profile, 'spec_sha256': digest(spec), 'catalogue_sha256': digest(catalogue_path),
                       'suite_sha256': digest(__file__),
                       'fixture_tool_sha256': digest(Path(__file__).with_name('effect_fixture.py')),
                       'capture_tool_sha256': digest('tools/oracle/capture.py'), 'disc_sha256': disc_hash,
                       'source_movie_sha256': digest(str(source)+'.dtm')}
        reference_dir = args.reference/name if args.reference else out
        if args.reference:
            recorded = json.loads((reference_dir/'reference.json').read_text())
            if (recorded['inputs'] != fingerprint
                    or digest(reference_dir/'dolphin/reference.png') != recorded['image_sha256']
                    or digest(reference_dir/'dolphin/capture.json') != recorded['capture_sha256']):
                raise ValueError('cached reference provenance differs: '+name)
        else:
            matte_position, _ = plane(camera, 320, 240, 3)
            matte = {'position': matte_position, 'rotation': [0, 0, 0], 'size': [100000, 100000],
                     'rgba': [0, 255, 0, 255] if color[1] else [0, 0, 255, 255],
                     'world_space': False, 'blend': 0, 'uv_bytes': [185, 31, 0, 0], 'texture': 2}
            fixture = out/'fixture.s01'
            models = [p for p in probes if p['shape']['kind'] == 'model']
            model = {**models[0], 'scale': models[0]['shape']['scale']} if models else None
            edits = prepare(source, fixture, observation, [matte]+[source_sprite(p, catalogue) for p in probes if p['shape']['kind'] != 'model'], model)
            write(out/'fixture.json', edits)
            movie = out/'neutral.dtm'
            run_command(['target/debug/resonance-oracle', 'dtm', inputs, '--prefix', str(fixture)+'.dtm',
                         '--start-poll', observation['movie']['input_count'], '--output', movie], out/'movie.log')
            run_command(['python3', 'tools/oracle/capture.py', '--disc', args.disc, '--movie', movie,
                         '--initial-state', fixture, '--output', out/'dolphin', '--frame', FRAME,
                         '--xvfb', '--watch-state', '--timeout', 90], out/'dolphin.log')
            write(out/'reference.json', {'inputs': fingerprint, 'image_sha256': digest(out/'dolphin/reference.png'),
                                        'capture_sha256': digest(out/'dolphin/capture.json')})
        metadata = json.loads((reference_dir/'dolphin/capture.json').read_text())
        if (not metadata['complete'] or metadata['disc_sha256'] != disc_hash
                or metadata['requested_frame'] != FRAME
                or metadata['dolphin_version'] != 'Dolphin [master] 2606'
                or not metadata['presentation']['gecko_verified']
                or metadata['audio']['backend'] != 'No Audio Output'):
            raise ValueError('invalid Dolphin capture provenance: '+name)
        run_command([args.native, spec, out/'native', args.cooked], out/'native.log')
        reference = Image.open(reference_dir/'dolphin/reference.png')
        actual = Image.open(out/'native'/f'frame-{FRAME:04}.png')
        background = Image.new('RGB', reference.size, tuple(color))
        actual_background = background
        if name.startswith('refraction-'):
            validate_checker(reference)
            validate_checker(actual)
        if name.startswith('refraction-') and name != 'refraction-background':
            background = Image.open(args.output/'refraction-background/reference.png')
            actual_background = Image.open(args.output/'refraction-background/actual.png')
        elif not name.startswith('refraction-'):
            validate_matte(reference, color)
            validate_matte(actual, color)
        delta = np.abs(np.asarray(reference.convert('RGB'), dtype=np.int16)-np.asarray(actual.convert('RGB'), dtype=np.int16))
        Image.fromarray(np.clip(delta*4, 0, 255).astype(np.uint8)).save(out/'difference.png')
        reference.save(out/'reference.png'); actual.save(out/'actual.png')
        for region in regions:
            results.append({'case': name+'/'+region['name'], **compare(reference, actual, background, region['rect'], actual_background)})
        print(name, sum(r['passed'] for r in results if r['case'].startswith(name+'/')), '/', len(regions), flush=True)
    if not results:
        raise ValueError('no cases selected')
    write(args.output/'results.json', {'gates': GATES, 'native_sha256': digest(args.native), 'results': results,
                                      'passed': all(r['passed'] for r in results), 'reference': str(args.reference) if args.reference else None})
    page='<html><meta charset="utf-8"><title>Effect primitive deltas</title><style>body{font:16px system-ui;background:#eee;margin:2rem}img{width:32%}td,th{padding:.4rem;border:1px solid #aaa}table{border-collapse:collapse}.fail{background:#ffd4d4}.pass{background:#d4ffda}</style><h1>Effect primitive deltas</h1><p>Reference / Resonance / absolute difference ×4. Fixed camera and samples; no image alignment or brightness fitting. Atlas poses are sampled from cooked recipes; emitter timing is outside this suite.</p>'
    page+=f'<p>{sum(r["passed"] for r in results)} / {len(results)} checks passed. Gates: {html.escape(json.dumps(GATES))}</p>'
    for name in dict.fromkeys(r['case'].split('/')[0] for r in results):
        page+='<h2>'+html.escape(name)+'</h2>'+''.join(f'<a href="{name}/{f}.png"><img src="{name}/{f}.png"></a>' for f in ['reference','actual','difference'])
    page+='<table><tr><th>Case</th><th>Result</th><th>Mean error</th><th>Changed fraction</th><th>Bounds delta</th></tr>'
    for r in results:
        page+=f'<tr class="{"pass" if r["passed"] else "fail"}"><td>{html.escape(r["case"])}</td><td>{"PASS" if r["passed"] else "FAIL"}</td><td>{r["mean_error"]}</td><td>{r["changed_fraction"]}</td><td>{r["bounds_delta"]}</td></tr>'
    (args.output/'report.html').write_text(page+'</table></html>')
    return 0 if all(r['passed'] for r in results) else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case', type=Path, default=Path('tools/oracle/cases/effect-bases.json'))
    parser.add_argument('--disc', type=Path, required=True)
    parser.add_argument('--cooked', type=Path, default=Path('local/all-assets'))
    parser.add_argument('--native', type=Path, default=Path('target/debug/examples/field_sequence'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--only')
    sys.exit(run(parser.parse_args()))
