import copy
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from PIL import Image, ImageDraw

from effect_lifecycle import assess, failure_signature, compare_frame, completed_capture, controller_inputs, ControllerMovies, presentation_delay, validate_capture, capture_identity, run, profile_cases, PROFILES, DOLPHIN_OPTIONS, SYNC_WHITE, PRESENTATION_FLUSH
from effect_cases import cases, minimize, random_cases, scenario_commands
from effect_fixture import prepare, SPRITE_POOL, MODEL_POOL, SPRITE_STRIDE
from effect_workers import DOLPHIN_BACKEND, DolphinFixture, register_boundary, renderer_environment, set_movie_backend


class LifecycleComparison(unittest.TestCase):
    def test_capture_discards_only_a_verified_paused_image(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory)/str(i) for i in range(4)]
            for path, content in zip(paths, [b'paused', b'paused', b'first', b'second']):
                path.write_bytes(content)
            rows = [{'vi': 1}, {'vi': 2}]
            self.assertEqual(register_boundary(paths[1:], rows, paths[0]),
                             (paths[2:], rows, 0, True))
            self.assertEqual(register_boundary(paths[2:], rows, paths[0]),
                             (paths[2:], rows, 0, False))
            for paused in (None, paths[3]):
                with self.assertRaises(RuntimeError):
                    register_boundary(paths[1:], rows, paused)

    def test_failed_capture_keeps_raw_images_for_diagnosis(self):
        for complete in (False, True):
            with tempfile.TemporaryDirectory() as directory:
                worker = DolphinFixture.__new__(DolphinFixture)
                worker.output = Path(directory)
                worker.user = worker.output/'user'
                raw = worker.user/'Dump/Frames/frame.png'
                raw.parent.mkdir(parents=True)
                raw.write_bytes(b'capture evidence')
                worker.process = worker.display = worker.watcher = worker.alias = None
                worker.metadata = {'complete': complete}
                worker.close(complete=complete)
                self.assertEqual(raw.exists(), not complete)

    def test_controller_reduction_records_new_inputs_and_reuses_repeated_candidates(self):
        camera = {'position': [700, -324, 736], 'target': [0, 97, 87]}
        case = next(cases(camera, 'rings'))
        case['inputs'] = [{'poll': 100, 'duration': 10, 'buttons': ['x']}]
        def record(command, log):
            Path(command[-1]).write_bytes(b'DTM\x1a' + bytes(252) + Path(command[2]).read_bytes())
        with tempfile.TemporaryDirectory() as directory, patch('effect_lifecycle.run_command', side_effect=record) as recorder:
            movies = ControllerMovies(Path('source.s01'), 200, Path(directory))
            original = movies.get(case['inputs'])
            def fails(candidate):
                movies.get(candidate['inputs'])
                return True
            reduced = minimize(case, fails)
            calls = recorder.call_count
            movie, fingerprint = movies.get(reduced['inputs'])
            self.assertEqual(recorder.call_count, calls)
            self.assertNotEqual((movie, fingerprint), original)
            self.assertEqual(json.loads(movie.read_bytes()[256:])['inputs'], reduced['inputs'])
            self.assertEqual(reduced['inputs'][0], {'poll': 60, 'duration': 2, 'buttons': ['x']})

    def test_movie_renderer_selection_preserves_controller_recording_and_other_settings(self):
        original = b'DTM\x1a' + bytes(range(252)) + b'recorded controller input'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'input.dtm'
            path.write_bytes(original)
            set_movie_backend(path)
            movie = path.read_bytes()
            self.assertEqual(movie[:81], original[:81])
            self.assertEqual(movie[81:97].rstrip(b'\0'), b'Vulkan')
            self.assertEqual(movie[97:], original[97:])
            path.write_bytes(b'invalid')
            with self.assertRaises(ValueError):
                set_movie_backend(path)

    def test_checker_tiles_have_the_same_state_with_or_without_a_matte(self):
        tile = dict(position=[0, 0, 0], rotation=[0, 0, 0], size=[80, 80],
                    rgba=[64, 64, 64, 255], world_space=False, blend=0,
                    uv_bytes=[185, 31, 0, 0], texture=2)
        matte = dict(tile, size=[100000, 100000], rgba=[0, 0, 0, 255])
        states = []
        for background in (None, matte):
            memory = {}
            source = Mock(path=Path('source'), sha256='source', changes=[])
            source.fork.return_value = source
            source.read.side_effect = lambda address, fmt='I': {
                0x8035A578: 0x80400000, 0x80405820: 0x80500000,
                SPRITE_POOL: 0x80600000, MODEL_POOL: 0x80700000,
            }.get(address, 0)
            source.write.side_effect = lambda address, fmt, value: memory.update({address: value})
            with patch('effect_fixture.digest', return_value='fixture'), patch('effect_fixture.shutil.copyfile'):
                prepare(source, Path('fixture'), {'actors': []}, [tile], seed=None, matte=background)
            start = 0x80600000 + (SPRITE_STRIDE if background else 0)
            states.append({address-start: value for address, value in memory.items()
                           if start <= address < start+SPRITE_STRIDE})
        self.assertEqual(*states)

    def test_default_suite_covers_all_stages_and_runs_the_requested_random_total(self):
        camera = {'position': [700, -324, 736], 'target': [0, 97, 87]}
        names, modes = [], set()
        for path in PROFILES:
            for case in profile_cases(json.loads(path.read_text()), camera):
                names.append(case['name'])
                modes.update(tuple(args) for op, args in scenario_commands(case) if op == 0x78)
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual({mode for mode, _ in modes}, set(range(1, 20)))
        self.assertTrue({'renegade-shot', 'colette-wings', 'kratos-wings', 'yggdrasil-wings',
                         'eternal-sword', 'eternal-sword-halo', 'station'} <= set(names))
        self.assertTrue({f'bound-sprite-{slot}' for slot in range(8)} <= set(names))
        counts = []
        def capture(stage):
            counts.append(stage.random_cases)
            bases = profile_cases(json.loads(stage.case.read_text()), camera)
            identity = lambda effect: (effect['kind'], json.dumps(effect['variant']))
            generated = list(random_cases(camera, bases, stage.random_cases, stage.seed))
            self.assertEqual({identity(c['effects'][0]) for c in generated},
                             {identity(c['effects'][0]) for c in bases})
            stage.output.mkdir()
            (stage.output/'results.json').write_text(json.dumps({
                'results': [{}], 'passed': True, 'source_complete': True}))
            return 0
        with tempfile.TemporaryDirectory() as directory, patch('effect_lifecycle.run_profile', capture):
            args = SimpleNamespace(case=None, workers=2, random_cases=500, replay_case=None,
                                   resume=False, reference=None, output=Path(directory)/'suite',
                                   seed=17, only=None)
            self.assertEqual(run(args), 0)
            self.assertEqual(len(counts), len(PROFILES))
            self.assertEqual(sum(counts), 500)
            self.assertTrue(all(counts))

    def test_ring_and_model_generation_changes_the_primary_subject(self):
        camera = {'position': [700, -324, 736], 'target': [0, 97, 87]}
        for path in PROFILES[1:]:
            bases = profile_cases(json.loads(path.read_text()), camera)
            for base in bases:
                generated = list(random_cases(camera, [base], 20, 17))
                variants = {json.dumps((case['effects'][0], case.get('inputs', []))) for case in generated}
                self.assertGreater(len(variants), 1, base['name'])
                if base['inputs']:
                    self.assertGreater(len({json.dumps(case['inputs']) for case in generated}), 1)
                    for case in generated:
                        controller_inputs(case['inputs'])

    def test_filtered_suite_replay_visits_only_recorded_profiles(self):
        def capture(stage):
            if stage.reference:
                self.assertTrue((stage.reference/'inputs.json').is_file())
            if stage.case.stem != 'effect-tower':
                return None
            stage.output.mkdir()
            (stage.output/'inputs.json').write_text('{}')
            (stage.output/'results.json').write_text(json.dumps({
                'results': [{}], 'passed': True, 'source_complete': True}))
            return 0
        with tempfile.TemporaryDirectory() as directory, patch('effect_lifecycle.run_profile', capture):
            args = SimpleNamespace(case=None, workers=1, random_cases=0, replay_case=None,
                                   resume=False, reference=None, output=Path(directory)/'source',
                                   seed=17, only='renegade-shot')
            self.assertEqual(run(args), 0)
            args.reference, args.output = args.output, Path(directory)/'replay'
            self.assertEqual(run(args), 0)

    def test_resume_keeps_the_seed_and_random_count_for_unstarted_profiles(self):
        observed = []
        def capture(stage):
            observed.append((stage.case, stage.seed, stage.random_cases))
            stage.output.mkdir(exist_ok=stage.resume)
            (stage.output/'inputs.json').write_text('{}')
            (stage.output/'results.json').write_text(json.dumps({
                'results': [{}], 'passed': True, 'source_complete': True}))
            return 0
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(case=None, workers=1, random_cases=500, replay_case=None,
                                   resume=False, reference=None, output=Path(directory)/'suite',
                                   seed=17, only=None)
            def interrupted(stage):
                if len(observed) == 2:
                    raise InterruptedError()
                return capture(stage)
            with patch('effect_lifecycle.run_profile', interrupted), self.assertRaises(InterruptedError):
                run(args)
            args.resume, args.random_cases, args.seed = True, 0, 99
            with patch('effect_lifecycle.run_profile', capture):
                self.assertEqual(run(args), 0)
            self.assertEqual(sum(count for _, _, count in observed), 500)
            self.assertEqual({seed for _, seed, _ in observed}, {17})
            self.assertEqual({path for path, _, _ in observed}, set(PROFILES))

    def test_held_controller_input_uses_the_same_window_in_both_engines(self):
        self.assertEqual(controller_inputs([
            {'poll': 100, 'duration': 120, 'buttons': ['x']},
            {'poll': 300, 'duration': 10, 'buttons': ['x']},
        ]), [{'update': 50, 'buttons': ['ring']}, {'update': 110, 'buttons': []},
             {'update': 150, 'buttons': ['ring']}, {'update': 155, 'buttons': []}])
        with self.assertRaises(ValueError):
            controller_inputs([{'poll': 1, 'duration': 2, 'buttons': ['x']}])

    def test_cached_frames_are_reusable_only_for_the_same_inputs_and_a_complete_window(self):
        inputs = {'source': {'path': 'old/location.s01', 'sha256': 'state'}, 'disc': 'disc'}
        moved = dict(inputs, source={'path': 'fixtures/base.s01', 'sha256': 'state'})
        self.assertEqual(capture_identity(inputs), capture_identity(moved))
        self.assertNotEqual(capture_identity(inputs), capture_identity(dict(moved, disc='changed')))
        moved['source']['sha256'] = 'changed'
        self.assertNotEqual(capture_identity(inputs), capture_identity(moved))
        capture = {'complete': True, 'presentation': {'one_image_per_vi': True},
                   'renderer': {'backend': DOLPHIN_BACKEND, 'environment': renderer_environment()},
                   'command': ['dolphin'] + DOLPHIN_OPTIONS, 'movie_sha256': 'movie',
                   'samples': [{'state_sha256': 'state', 'frames': 10,
                                'images': {f'frame-{i:04}.png': 'hash' for i in range(10)}}]}
        fixture = {'fixture_sha256': 'state'}
        self.assertEqual(validate_capture(capture, fixture, 'movie', 8)['frames'], 10)
        for changed, movie, state, frames in [
            ({'complete': False}, 'movie', 'state', 8),
            ({'command': ['dolphin', '-v', 'other']}, 'movie', 'state', 8),
            ({'renderer': {'backend': 'OGL', 'environment': renderer_environment()}}, 'movie', 'state', 8),
            ({'renderer': {'backend': DOLPHIN_BACKEND, 'environment': {}}}, 'movie', 'state', 8),
            ({}, 'other movie', 'state', 8), ({}, 'movie', 'other state', 8),
            ({}, 'movie', 'state', 11),
        ]:
            with self.assertRaises(ValueError):
                validate_capture(capture | changed, {'fixture_sha256': state}, movie, frames)
        missing = copy.deepcopy(capture)
        del missing['samples'][0]['images']['frame-0004.png']
        with self.assertRaises(ValueError):
            validate_capture(missing, fixture, 'movie', 8)

    def test_both_engines_remaining_visible_is_a_fixture_error_not_an_expiry_mismatch(self):
        image = Image.new('RGB', (640, 480), (80, 80, 80))
        frame = compare_frame(image, image)
        case = {'expires': True}
        result = {'source_complete': True, 'frames': [frame], **assess(case, [frame])}
        self.assertTrue(frame['passed'])
        self.assertFalse(result['passed'])
        self.assertIn('extend', result['fixture_error'])
        self.assertIsNone(failure_signature(case, result))
        frame = compare_frame(Image.new('RGB', image.size), image)
        frames = [compare_frame(image, image)] + [frame]*5
        result = {'source_complete': True, 'frames': frames, **assess(case, frames)}
        self.assertIsNone(result['fixture_error'])
        self.assertEqual(failure_signature(case, result), 'expiry')

    def test_empty_recording_requires_an_explicit_invisible_subject(self):
        empty = Image.new('RGB', (640, 480))
        frames = [compare_frame(empty, empty)]*60
        for case in ({'expires': True}, {'expires': True, 'visible': True}):
            result = assess(case, frames)
            self.assertFalse(result['passed'])
            self.assertIn('visibility', result['fixture_error'])
        self.assertTrue(assess({'expires': True, 'visible': False}, frames)['passed'])

    def test_matching_clipped_effects_cannot_validate_a_geometry_baseline(self):
        image = Image.new('RGB', (640, 480), (80, 80, 80))
        clipped = compare_frame(image, image)
        empty = Image.new('RGB', image.size)
        frames = [clipped] + [compare_frame(empty, empty)]*5
        case = next(cases({'position': [700, -324, 736], 'target': [0, 97, 87]}))
        result = assess(case, frames)
        self.assertTrue(clipped['passed'])
        self.assertFalse(result['passed'])
        self.assertIn('bounds', result['fixture_error'])

        # Framing depends on source coverage even when the missing edge pixel
        # is within the permitted color-rounding tolerance.
        reference = empty.copy()
        ImageDraw.Draw(reference).rectangle((300, 200, 309, 209), fill=(100,)*3)
        actual = reference.copy()
        reference.putpixel((0, 200), (4,)*3)
        clipped = compare_frame(reference, actual)
        self.assertTrue(clipped['passed'])
        self.assertIn('bounds', assess(case, [clipped] + frames[1:])['fixture_error'])

    def test_resume_recaptures_an_interrupted_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            for contents in [None, '{"complete":', '{"complete": false}']:
                if contents is not None:
                    (path/'capture.json').write_text(contents)
                self.assertFalse(completed_capture(path))
            (path/'capture.json').write_text('{"complete": true}')
            self.assertTrue(completed_capture(path))

    def test_clock_registration_uses_only_the_fixture_marker_and_rejects_a_dropped_frame(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            for frame in range(30 + PRESENTATION_FLUSH):
                level = 80 if frame-2 in SYNC_WHITE else 0
                Image.new('RGB', (640, 480), (level,)*3).save(path/f'frame-{frame:04}.png')
            self.assertEqual(presentation_delay(path), 2)
            Image.new('RGB', (640, 480)).save(path/'frame-0014.png')
            with self.assertRaises(ValueError):
                presentation_delay(path)
            for frame in range(30):
                Image.new('RGB', (640, 480), (80 if frame in SYNC_WHITE else 0,)*3).save(path/f'frame-{frame:04}.png')
            for frame in range(30, 30 + PRESENTATION_FLUSH):
                (path/f'frame-{frame:04}.png').unlink()
            self.assertEqual(presentation_delay(path, maximum=0), 0)

    def test_refraction_comparison_excludes_static_scenery_but_detects_missing_distortion(self):
        background = Image.new('RGB', (640, 480), (80, 80, 80))
        distorted = background.copy()
        distorted.putpixel((320, 240), (180, 180, 180))
        self.assertEqual(compare_frame(background, background, background)['reference_active_pixels'], 0)
        self.assertTrue(compare_frame(distorted, distorted, background)['passed'])
        self.assertFalse(compare_frame(distorted, background, background)['passed'])
