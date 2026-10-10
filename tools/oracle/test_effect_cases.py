import copy
import json
import math
import random
import unittest
from unittest.mock import patch

from effect_cases import FIELD_TYPES, cases, random_cases, scenario_commands, minimize, composed_case, capture_settings, effect, effect_events, emitter_vector_slot
from effect_fixture import fixture_program, SCENE_SETUP

CAMERA = {'position': [700, -324, 736], 'target': [0, 97, 87]}


class GeneratedEffects(unittest.TestCase):
    def test_distant_camera_keeps_each_layer_and_backdrop_inside_the_far_plane(self):
        camera = {'position': [2547, -127, 1243], 'target': [1550, 980, 79]}
        distance = math.dist(camera['position'], camera['target'])
        forward = [(t-p)/distance for p, t in zip(camera['position'], camera['target'])]
        for kind, variant in FIELD_TYPES:
            subject = effect(kind, variant, camera, random.Random(17))
            with self.subTest(kind=kind, variant=variant):
                depth = sum((p-e)*f for p, e, f in zip(subject['position'], camera['position'], forward))
                self.assertTrue(100 < depth < 40000)
                settings = capture_settings(composed_case('distant', [subject], 1))
                self.assertTrue(depth < settings['backdrop_depth'] * distance < 40000)

    def assert_lifetimes(self, commands):
        live, emitters = set(), set()
        for op, args in commands:
            if op in (0x10, 0x5c, 0xbf):
                self.assertNotIn(args[0], live)
                live.add(args[0])
                if op in (0x5c, 0xbf):
                    emitters.add(args[0])
            elif op == 0x12:
                self.assertIn(args[0], live)
                live.remove(args[0])
                emitters.discard(args[0])
        self.assertFalse(emitters)

    def test_changing_the_catalogue_does_not_change_existing_baselines(self):
        before = {case['name']: case for case in cases(CAMERA)}
        with patch('effect_cases.FIELD_TYPES', list(reversed(FIELD_TYPES))[1:]):
            after = {case['name']: case for case in cases(CAMERA)}
        self.assertEqual(after, {name: before[name] for name in after})

    def test_model_ring_and_station_compositions_keep_every_actor_and_emitter_independent(self):
        variants = set()
        for family in ('tower', 'rings', 'stations'):
            bases = list(cases(CAMERA, family))
            for case in random_cases(CAMERA, bases, 500, 20261006):
                commands = list(scenario_commands(case))
                self.assert_lifetimes(commands)
                variants.update(args[5] for op, args in commands if op == 0xbf)
                if family == 'stations':
                    self.assertTrue(any(op == 0x5c for op, _ in commands))
        self.assertTrue({40, 47, 50, 66, 70} <= variants)

    def test_visibility_tracks_palette_and_emission_changes(self):
        bases = {c['name']: c for c in cases(CAMERA)}
        for kind in (9, 13, 15, 16, 34, 54, 75, 48):
            case = bases[f'emitter-{kind}']
            effect = case['effects'][0]
            name, hidden, visible = ('interval', 0, 1) if kind == 48 else ('palette', 105, 33)
            effect['changes'] = [[t, values] for t, values in effect['changes'] if name not in values]
            effect['parameters'][name] = hidden
            self.assertFalse(capture_settings(case)['visible'])
            effect['changes'].append([effect['birth'] + 1, {name: visible}])
            self.assertEqual(capture_settings(case)['visible'], kind not in (16, 34))
            case['visible'] = False
            self.assertFalse(capture_settings(case)['visible'])

    def test_generated_compositions_are_replayable_and_cover_effect_inputs(self):
        bases = list(cases(CAMERA))
        generated = list(random_cases(CAMERA, bases, 500, 17))
        self.assertEqual(generated, list(random_cases(CAMERA, bases, 500, 17)))
        self.assertNotEqual(generated[0], next(random_cases(CAMERA, bases, 1, 18)))
        variants, pairs, model_properties, palettes, seal_phases = set(), set(), set(), set(), set()
        gathering_phases, mote_phases = set(), set()
        moving_sprites = reused_handles = target_changes = 0
        sprite_properties = set()
        for case in generated:
            commands = list(scenario_commands(case))
            self.assertEqual(commands, list(scenario_commands(json.loads(json.dumps(case)))))
            fixture_program([0, 90020, 32, 33, 34, 10000, 10001], SCENE_SETUP + commands)
            handles = [args[0] for op, args in commands if op == 0xbf]
            self.assertFalse(set(handles) & {args[0] for op, args in commands if op == 0xb2},
                             'face controls overwrite emitter parameters')
            reused_handles += len(handles) - len(set(handles))
            self.assert_lifetimes(commands)
            moving_sprites += sum(op in (0xd0, 0xd3) and any(args[5:8])
                                  and (op == 0xd0 or args[8] != 0) for op, args in commands)
            model_properties.update(args[1] for op, args in commands if op == 0xde)
            sprite_properties.update(args[1] for op, args in commands if op == 0xd1)
            for e in case['effects']:
                variants.add((e['kind'], e['variant']))
                if e['kind'] == 'sprite':
                    p = e['parameters']
                    for tick, _ in e['changes']:
                        age = tick - e['birth']
                        self.assertLess(age, p['lifetime'])
                        self.assertGreater(p['alpha'] + p['fade'] * age, 0)
                if e['kind'] == 'emitter':
                    if 'target' in e['parameters']:
                        slot = 113 + emitter_vector_slot(e['variant'], 'target')
                        targets = [args[2] for _, op, args in effect_events(e)
                                   if op == 0x1d and slot <= args[1] < slot+3]
                        self.assertTrue(all(0 <= value <= 0xffff for value in targets),
                                        'target edits must stay in the unsigned coordinate range')
                        target_changes += len(targets)
                    palettes.add(e['parameters'].get('palette'))
                    if e['variant'] == 11:
                        gathering_phases.update(values['phase'] for _, values in e['changes'] if 'phase' in values)
                    if e['variant'] == 49:
                        seal_phases.update(values['phase'] for _, values in e['changes'] if 'phase' in values)
                    if e['variant'] == 18:
                        mote_phases.update(values['phase'] for _, values in e['changes'] if 'phase' in values)
                        for values in [e['parameters']] + [values for _, values in e['changes']]:
                            if 'target' in values:
                                self.assertGreaterEqual(math.dist(e['position'], values['target']) / e['speed'], 3)
            emitters = tuple(e['variant'] for e in case['effects'] if e['kind'] == 'emitter')
            if len(emitters) > 1:
                pairs.add(emitters)
        # Required inputs are explicit, independent of the generator's catalogue.
        self.assertTrue({('emote', n) for n in range(20)} <= variants)
        self.assertTrue({('sprite', n) for n in (0, 1, 2, 4, 5, 6, 7, 8, 10, 14, 23, 25,
                                                27, 28, 40, 41, 42, 43, 49, 52, 53, 54, 69,
                                                11, 12, 13, 18, 21, 22, 70, 74, 80, 501, 502, 503, 504, 505)} <= variants)
        self.assertTrue({105, 106, 107, 108} <= palettes)
        self.assertEqual(seal_phases, {1, 3})
        self.assertEqual(gathering_phases, {0, 1, 2})
        mote = next(c for c in bases if c['name'] == 'emitter-18')
        mote_phases.update(values['phase'] for case in random_cases(CAMERA, [mote], 20, 17)
                           for e in case['effects'] if e['kind'] == 'emitter' and e['variant'] == 18
                           for _, values in e['changes'] if 'phase' in values)
        self.assertIn(0, mote_phases, 'generated motes must exercise restart')
        self.assertTrue({('model', n) for n in range(3)} <= variants)
        emitters = set(range(5)) | set(range(9, 35)) | set(range(36, 69)) | set(range(70, 76))
        self.assertTrue({('emitter', n) for n in emitters - {40, 47, 50, 66, 70}} <= variants)
        self.assertTrue(pairs, 'generated cases must combine emitters')
        self.assertTrue(reused_handles, 'generated cases must recreate removed emitters')
        self.assertTrue(moving_sprites, 'generated cases must exercise moving sprites')
        self.assertTrue({123, 124, 128, 135, 145, 146, 147} <= sprite_properties)
        self.assertTrue(target_changes, 'generated cases must retarget moving effects')
        self.assertTrue(set(range(423, 426)) | set(range(429, 432)) | set(range(435, 441))
                        | {442, 443, 447, 448} <= model_properties)

    def test_quakes_are_removed_during_startup_and_active_shaking(self):
        base = next(c for c in cases(CAMERA) if c['name'] == 'emitter-22')
        runs = {c['effects'][0]['removal'] - c['effects'][0]['birth']
                for c in random_cases(CAMERA, [base], 20, 17)}
        self.assertTrue(any(run < 20 for run in runs))
        self.assertTrue(any(20 <= run < 140 for run in runs))

    def test_composition_does_not_delay_the_stage_or_alias_model_resources(self):
        bases = [c for c in cases(CAMERA) if c['name'] == 'model-0']
        case = next(random_cases(CAMERA, bases, 1, 9))
        ring = next(cases(CAMERA, 'rings'))['effects'][0]
        ring['birth'] = 50
        case['effects'].append(ring)
        tick, active, grant, ring = 0, False, None, None
        saved = set()
        for op, args in scenario_commands(case):
            if op == 'copy':
                saved.add(args[0])
            if op == 'clock':
                active = True
            elif active and op == 0x64:
                tick += args[1]
            elif op == 0x20:
                grant = tick
            elif op == 0x78:
                ring = tick
            elif op == 0xdd:
                self.assertIn(args[0]['variable'], saved)
        self.assertEqual((grant, ring), (50, 50))

    def test_shrinking_preserves_the_failing_effect_and_its_cleanup(self):
        case = next(random_cases(CAMERA, [c for c in cases(CAMERA) if c['name'] == 'emitter-38'], 1, 17))
        case['effects'][0]['parameters']['count'] = 12
        original = copy.deepcopy(case)
        def fails(candidate):
            return any(e['kind'] == 'emitter' and e['variant'] == 38 and
                       e['parameters']['count'] >= 2
                       for e in candidate['effects'])
        reduced = minimize(case, fails)
        self.assertEqual(len(reduced['effects']), 1)
        effect = reduced['effects'][0]
        self.assertEqual(effect['parameters']['count'], 2)
        self.assertGreaterEqual(effect['removal'], effect['birth'])
        self.assertEqual(original, case)

    def test_projectile_speed_varies_and_primary_models_can_be_shrunk_without_overlays(self):
        bases = {c['name']: c for c in cases(CAMERA, 'tower')}
        for name in ('emitter-40', 'emitter-47', 'emitter-70', 'renegade-shot'):
            commands = list(scenario_commands(bases[name]))
            binding = next(i for i, (op, _) in enumerate(commands) if op == 0xd2)
            birth = next(i for i, (op, _) in enumerate(commands) if op == 0xbf)
            self.assertLess(binding, birth, 'arrival flashes need an explicit texture binding')
        speeds = set()
        for case in random_cases(CAMERA, [bases['renegade-shot']], 20, 17):
            speed = None
            for op, args in scenario_commands(case):
                if op == 0xbf:
                    speed = args[7]
                elif op == 0x1d and args[1] == 5:
                    self.assertEqual(args[2], speed)
            speeds.add(speed)
        self.assertGreater(len(speeds), 1)
        case = next(random_cases(CAMERA, [bases['colette-wings']], 1, 17))
        case['effects'].append(next(c for c in cases(CAMERA) if c['name'] == 'sprite-0')['effects'][0])
        reduced = minimize(case, lambda c: any(e['kind'] == 'actor' for e in c['effects']))
        self.assertEqual([e['kind'] for e in reduced['effects']], ['actor'])
        self.assertTrue(any(op == 0x12 for op, _ in scenario_commands(reduced)))

    def test_shrinking_keeps_edits_before_particle_expiry(self):
        case = next(c for c in cases(CAMERA) if c['name'] == 'sprite-0')
        effect = case['effects'][0]
        effect.update(birth=0, changes=[[60, {'growth': 100}]])
        effect['parameters'].update(lifetime=90, alpha=255, fade=-2)
        reduced = minimize(case, lambda c: c['effects'][0]['changes'] == effect['changes'])
        p = reduced['effects'][0]['parameters']
        self.assertGreater(p['lifetime'], 60)
        self.assertGreater(p['alpha'] + p['fade'] * 60, 0)

    def test_shrinking_recalculates_visibility_backdrop_and_capture_length(self):
        bases = {c['name']: c['effects'][0] for c in cases(CAMERA)}
        case = composed_case('mixed', [bases['sprite-27'], bases['emitter-38'], bases['emote-9']], 1)
        reduced = minimize(case, lambda c: any(e['kind'] == 'emote' for e in c['effects']))
        before, after = capture_settings(case), capture_settings(reduced)
        self.assertTrue(before['visible'])
        self.assertTrue(before['checkerboard'])
        self.assertFalse(after['visible'])
        self.assertFalse(after['checkerboard'])
        self.assertLess(after['updates'], before['updates'])
        subject = reduced['effects'][0]
        self.assertGreater(after['updates'], reduced['warmup'] + subject['birth'] + subject['parameters']['lifetime'])

    def test_blend_changes_have_a_visible_destination_color(self):
        case = next(c for c in cases(CAMERA) if c['name'] == 'sprite-4')
        for blend in range(4):
            case['effects'][0]['changes'] = [[10, {'blend': blend}]]
            self.assertTrue(capture_settings(case)['checkerboard'])
        reduced = minimize(case, lambda c: bool(c['effects']))
        self.assertFalse(capture_settings(reduced)['checkerboard'])

    def test_shrinking_shortens_model_lifetime_without_inheriting_a_capture_budget(self):
        case = next(c for c in cases(CAMERA, 'tower') if c['name'] == 'colette-wings')
        case['updates'] = 1000
        def persists(candidate):
            return candidate['effects'][0]['removal'] >= 8
        reduced = minimize(case, persists)
        self.assertEqual((reduced['effects'][0]['birth'], reduced['effects'][0]['removal']), (0, 8))
        self.assertLess(capture_settings(reduced)['updates'], capture_settings(case)['updates'])
        self.assertLess(capture_settings(case)['updates'], 1000)

    def test_capture_includes_particle_lifetimes_from_constructor_and_later_changes(self):
        case = next(c for c in cases(CAMERA) if c['name'] == 'emitter-48')
        effect = case['effects'][0]
        removal = effect['removal']
        effect['parameters']['lifetime'] = 400
        self.assertGreater(capture_settings(case)['updates'], case['warmup'] + removal + 400)
        effect['changes'].append([removal-1, {'lifetime': 600}])
        self.assertGreater(capture_settings(case)['updates'], case['warmup'] + removal + 600)

    def test_cleanup_transitions_are_exercised_without_relying_on_random_sampling(self):
        switches = set()
        for case in cases(CAMERA):
            effect = case['effects'][0]
            if effect['kind'] != 'emitter' or effect['variant'] not in (38, 59):
                continue
            cleanup, phase = effect['parameters']['cleanup'], 0
            for _, changes in sorted(effect['changes'], key=lambda row: row[0]):
                phase = changes.get('phase', phase)
                if 'cleanup' in changes:
                    switches.add((effect['variant'], phase, cleanup, changes['cleanup']))
                    cleanup = changes['cleanup']
        self.assertTrue({(variant, phase, before, 1-before)
                         for variant, phase in [(38, 2), (59, 0), (59, 1), (59, 2)]
                         for before in (0, 1)} <= switches)
