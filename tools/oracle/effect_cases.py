"""Scenario inputs for independent effect simulations; no rendered particle poses."""
import copy
import math
import random

import numpy as np

from effect_compare import plane

# Three white presentations identify the input clock before effects begin.
SYNC_WHITE = {8, 11, 12}
SYNC = [[0x64, [0, 8]], [0x63, [3, 0]], [0x64, [0, 1]],
        [0x63, [0, 0]], [0x64, [0, 2]], [0x63, [3, 0]],
        [0x64, [0, 2]], [0x63, [0, 0]], [0x64, [0, 17]], ['clock', []]]

# Finite input domains at the scenario boundary. Slots are constructor parameters,
# not particle state. Small counts and lifetimes keep compositions inside the pool.
PALETTES = (0, 33, 48)
RANDOM_PALETTES = (105, 106, 107, 108)
SIZES = (8, 16, 32)
SPRITE_PIXELS = (16, 24, 32)
REFRACTION_PIXELS = (64, 80, 96)
LIFETIMES = (1, 2, 30, 60, 90)
INVISIBLE_EMOTES = {9, 16, 17, 18, 19}
EMPTY_PALETTES = tuple(range(105, 110))
ALPHAS = (32, 128, 200, 255)
VISIBLE_ALPHAS = (128, 160, 200, 255)
VELOCITIES = (-2, -1, 0, 1, 2)
SPEEDS = (0, 99, 100, 101, 200)
FADES = (0, -2, -8)
SMOKE_UPDATES = 56
MODEL_PROPERTIES = {
    **{f'velocity_{axis}': (423+i, (-200, -100, 0, 100, 200)) for i, axis in enumerate('xyz')},
    **{f'spin_{axis}': (429+i, (-300, -100, 0, 100, 300)) for i, axis in enumerate('xyz')},
    **{f'growth_{axis}': (435+i, (-1, 0, 1)) for i, axis in enumerate('xyz')},
    **{f'tint_{axis}': (438+i, (32, 64, 128)) for i, axis in enumerate('rgb')},
    'speed': (442, SPEEDS), 'directed': (443, (0, 1)),
    'blend': (447, (0, 1, 2)), 'gravity_and_fade': (448, (0, 1, 2, 4, 8, 9, 10, 12)),
}
SPRITE_PROPERTIES = {
    'width': (123, SIZES), 'height': (124, SIZES),
    **{f'tint_{axis}': (125+i, (32, 64, 128)) for i, axis in enumerate('rgb')},
    'alpha': (128, ALPHAS),
    **{f'spin_{axis}': (132+i, (-300, 0, 300)) for i, axis in enumerate('xyz')},
    'growth': (135, (-100, 0, 100)),
    **{f'rotation_{axis}': (141+i, (0, 4500, 9000)) for i, axis in enumerate('xyz')},
    'orientation': (144, (0, 1)), 'blend': (145, (0, 1, 2, 3)),
    'fade_mode': (146, (8,)), 'anchor': (147, (0, 4, 8)), 'fog': (148, (0, 1)),
}
ACTOR_PROPERTIES = {'alpha': (8, VISIBLE_ALPHAS),
                    **{f'scale_{axis}': (30+i, (80, 100, 120)) for i, axis in enumerate('xyz')}}
INTERVALS = (1, 3, 8)


class Emitter:
    def __init__(self, fields, *, speeds=(0,), phases=((),), tail=301, fixed_palette=False, depth=2, falling=False, refracts=False, minimum_run=0):
        self.fields = {name: values for _, name, values in fields}
        self.slots = {name: slot for slot, name, _ in fields}
        self.speeds, self.phases = speeds, phases
        self.tail, self.fixed_palette, self.depth = tail, fixed_palette, depth
        self.falling = falling
        self.refracts = refracts
        self.minimum_run = minimum_run


# Each family declares its input domains, transitions and finite drain time together.
# These are oracle inputs, independent of the engine's effect implementation.
EMITTERS = {kind: definition for kinds, definition in [
    ((0, 1, 2, 3), Emitter([(0, 'size', (24, 40, 60))], phases=((2, 0),), tail=61)),
    ((4,), Emitter([], phases=((2, 1, 2, 3),), tail=121, depth=6)),
    ((10, 61), Emitter([(0, 'curvature', (50, 100, 500)), (1, 'size', (30, 60, 100)),
                (3, 'fade_sixteenths', (-64, -200, -400)), (7, 'launch_x', (100,)),
                (8, 'launch_y', (-50, 0, 50)), (9, 'launch_z', (0, 50))], speeds=(5, 10, 20), tail=81)),
    ((40,), Emitter([(0, 'curvature', (50, 100, 500)), (1, 'size', (30, 60, 100)),
                (3, 'fade_sixteenths', (-64, -200, -400)), (7, 'launch_x', (100,)),
                (8, 'launch_y', (-50, 0, 50)), (9, 'launch_z', (0, 50))], speeds=(5, 10, 20), tail=81, depth=6)),
    ((20,), Emitter([], tail=301, depth=6)),
    ((21,), Emitter([], tail=601, depth=6)),
    ((25,), Emitter([(0, 'size', (30, 60, 100)), (1, 'palette', PALETTES), (9, 'cleanup', (0, 1))], tail=101)),
    ((29,), Emitter([(3, 'radius', (50, 100, 150)), (4, 'spread', (10, 20, 30)),
                (5, 'tilt', (0, 15, 30))], tail=301, depth=6)),
    ((32,), Emitter([(0, 'palette', PALETTES), (1, 'size', (50, 100, 200))], speeds=(1, 2, 3), tail=61)),
    ((37,), Emitter([(0, 'curvature', (50, 100, 200)), (1, 'palette', PALETTES),
                (2, 'size', (30, 60, 100)), (7, 'launch_x', (100,)),
                (8, 'launch_y', (-50, 0, 50)), (9, 'launch_z', (0, 50))], speeds=(5, 10, 20), tail=61)),
    ((39,), Emitter([(0, 'palette', PALETTES), (1, 'lifetime', (30, 60, 90)),
                (2, 'radius', (30, 80, 150))], tail=0, depth=6)),
    ((41,), Emitter([(0, 'count', (3, 6, 12)), (1, 'lifetime', (30, 60, 90)),
                (2, 'width', (0, 10, 20)), (3, 'height', (0, 10, 20)), (4, 'height_step', (0, 5, 10)),
                (5, 'size', (20, 40, 60)), (6, 'size_variation', (1, 5, 10)),
                (7, 'alpha', (100, 150, 200)), (8, 'alpha_variation', (1, 20, 40))], speeds=(50, 100, 200), tail=0)),
    ((42,), Emitter([(1, 'size', (60, 100, 150)), (2, 'satellite_size', (8, 16, 24)),
                (3, 'yaw', (0, 45, 90)), (4, 'tilt', (0, 45, 90))], tail=2)),
    ((43,), Emitter([(0, 'palette', PALETTES), (1, 'size', (10, 20, 30)),
                (2, 'size_variation', (1, 5, 10)), (3, 'count', (3, 6, 12)),
                (4, 'color_group', (1, 2, 4))], speeds=(1, 2, 3), tail=301, depth=6)),
    ((44,), Emitter([(0, 'palette', PALETTES), (1, 'radius', (30, 60, 100)), (2, 'size', SIZES),
                (3, 'size_variation', (1, 8, 20)), (4, 'lifetime', (30, 60, 90)),
                (5, 'speed_variation', (1, 100, 200)), (6, 'alpha', VISIBLE_ALPHAS),
                (7, 'fade', FADES), (8, 'interval', INTERVALS)], speeds=(50, 100, 200), tail=0)),
    ((45,), Emitter([(0, 'palette', PALETTES)], tail=301, depth=6)),
    ((50,), Emitter([(1, 'duration', (15, 30, 60))], tail=61)),
    ((52,), Emitter([(0, 'lifetime', (10, 15, 20))], tail=301, depth=6)),
    ((53,), Emitter([], tail=11, refracts=True)),
    ((56,), Emitter([(0, 'palette', PALETTES), (1, 'size', (30, 60, 100)),
                (2, 'duration', (15, 30, 60)), (3, 'fade', (-2, -4, -8))], tail=61)),
    ((57,), Emitter([(0, 'radius', (30, 60, 100)), (1, 'lifetime', (30, 60, 90)),
                (2, 'size', SIZES), (3, 'size_variation', (1, 8, 20)), (4, 'alpha', VISIBLE_ALPHAS),
                (5, 'fade', FADES), (6, 'interval', INTERVALS), (7, 'spin', (0, 2, 5))], tail=0)),
    ((58,), Emitter([(1, 'size', (50, 100, 150)), (3, 'fade', (-5, -10, -20))],
                speeds=(5, 10, 20), phases=((1,),), tail=87, refracts=True)),
    ((59,), Emitter([(0, 'palette', PALETTES), (1, 'size', SIZES), (2, 'trail_size', SIZES),
                (3, 'lifetime', (30, 60, 90)), (4, 'trail_lifetime', (30, 60, 90)),
                (5, 'alpha', VISIBLE_ALPHAS), (6, 'trail_alpha', VISIBLE_ALPHAS),
                (7, 'fade_sixteenths', (-16, -32, -64)), (8, 'trail_fade_sixteenths', (0, -16, -32))],
                speeds=(3, 5, 8), phases=((1, 2),), tail=max(VISIBLE_ALPHAS)+1, depth=6)),
    ((62,), Emitter([(0, 'radius', (30, 60, 100)), (1, 'size', SIZES),
                (2, 'size_variation', (1, 8, 20))], tail=301, depth=4)),
    ((64,), Emitter([(0, 'radius', (30, 60, 100)), (1, 'size', SIZES),
                (2, 'size_variation', (1, 8, 20)), (3, 'interval', INTERVALS),
                (4, 'interval_variation', (1, 3, 5))], speeds=(1, 2, 3), tail=76)),
    ((65, 68), Emitter([(0, 'radius', (50, 100, 150)), (1, 'count', (4, 8, 12)), (2, 'size', SIZES),
                (3, 'size_variation', (1, 8, 20))], speeds=(5, 10, 20), phases=((1, 2),), tail=2, minimum_run=450)),
    ((67,), Emitter([(0, 'palette', PALETTES), (1, 'size', SIZES), (2, 'size_variation', (1, 8, 20)),
                (3, 'interval', INTERVALS), (7, 'radius', (30, 60, 100)),
                (8, 'radius_variation', (1, 10, 20)), (9, 'growth', (100, 200, 300))],
                speeds=(1, 2, 3), phases=((2,),), tail=201, depth=6)),
    ((70,), Emitter([(0, 'palette', PALETTES)], phases=((1,),), tail=61, depth=4)),
    ((71, 72), Emitter([(0, 'palette', PALETTES), (1, 'offset', (-10, 0, 10)), (2, 'lifetime', (0, 30, 60)),
                (3, 'width', (40, 60, 80)), (4, 'height', (160, 240, 320)), (5, 'alpha', VISIBLE_ALPHAS),
                (6, 'fade', FADES), (7, 'growth', (1, 3, 5)), (8, 'rate', (1, 3, 5))], tail=0, depth=4)),
    ((73,), Emitter([(0, 'palette', PALETTES), (1, 'radius', (30, 60, 100)), (2, 'size', SIZES),
                (3, 'size_variation', (1, 8, 20)), (4, 'fog', (0, 1)), (5, 'speed_variation', (1, 100, 200)),
                (6, 'alpha', VISIBLE_ALPHAS), (7, 'fade', (0, -1, -2)), (8, 'interval', INTERVALS)],
                speeds=(50, 100, 200), tail=301, depth=6)),
    ((74,), Emitter([(0, 'count', (3, 6, 12)), (1, 'lifetime', (30, 60, 90)),
                (2, 'lifetime_variation', (1, 5, 10)), (3, 'size', (30, 60, 100)),
                (4, 'size_variation', (1, 8, 20)), (5, 'speed_variation', (1, 100, 200)),
                (6, 'growth_variation', (1, 100, 200))], speeds=(50, 100, 200), tail=301, depth=4)),
    ((9, 75), Emitter([(0, 'palette', PALETTES + EMPTY_PALETTES), (1, 'radius', (0, 15, 30)),
               (2, 'lifetime', LIFETIMES), (3, 'interval', (3, 6, 12)),
               (4, 'angular_spacing', (30, 60, 120)), (5, 'size', SIZES),
               (6, 'alpha', ALPHAS), (7, 'rise', (0, 2, 5)),
               (8, 'speed', (0, 2, 4)), (9, 'size_variation', (0, 1, 5))], phases=((2, 0),), tail=0, fixed_palette=True, depth=16, falling=True)),
    ((11,), Emitter([(0, 'delay', (1, 15, 30))], phases=((2,), (1,), (1, 2, 0)), tail=121)),
    ((12,), Emitter([(0, 'palette', PALETTES), (1, 'size', SIZES), (9, 'cleanup', (0, 1))], tail=61)),
    ((13,), Emitter([(0, 'palette', PALETTES + (64, 65, 88, 106)), (1, 'width', SIZES), (2, 'width_variation', (1, 8)),
             (3, 'height', SIZES), (4, 'height_variation', (1, 8)),
             (5, 'lifetime', (30, 60, 90)), (6, 'trail_lifetime', (30, 60, 90))], speeds=(0, 3, 8, 12), phases=((1,),), tail=0, fixed_palette=True)),
    ((14,), Emitter([(0, 'palette', PALETTES), (1, 'size', SIZES)], tail=2)),
    ((15,), Emitter([(0, 'palette', PALETTES + EMPTY_PALETTES), (1, 'radius', (0, 30, 80)), (2, 'size', SIZES),
                (3, 'size_variation', (1, 8, 20)), (5, 'speed_variation', (1, 100, 300)),
                (8, 'interval', INTERVALS), (9, 'cleanup', (0, 1))], speeds=(50, 100, 200), fixed_palette=True, depth=8)),
    ((30,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'radius', (0, 30, 80)), (2, 'size', SIZES),
                (3, 'size_variation', (1, 8, 20)), (5, 'speed_variation', (1, 100, 300)),
                (8, 'interval', INTERVALS), (9, 'cleanup', (0, 1))], speeds=(50, 100, 200), depth=6)),
    ((16,), Emitter([(0, 'palette', PALETTES + EMPTY_PALETTES), (1, 'radius', (30, 60, 100)), (2, 'spread', (0, 10, 25))], tail=16, fixed_palette=True, depth=6)),
    ((17,), Emitter([], tail=121, depth=24, falling=True)),
    # Let the quake finish its camera envelope before removing its controller.
    ((22,), Emitter([], tail=41, refracts=True, minimum_run=141)),
    ((18,), Emitter([(0, 'curvature', (0, 50, 100, 200)), (1, 'size', SIZES)], speeds=(5, 10, 20), phases=((0,), (1,), (2,), (3,), (4,)), tail=61)),
    ((19,), Emitter([(0, 'palette', PALETTES), (1, 'duration', (1, 15, 40)),
             (2, 'travelling', (0, 1)), (3, 'radius', (10, 25, 50))], phases=((2,),), tail=9)),
    ((23,), Emitter([(0, 'palette', PALETTES), (1, 'size', (30, 60, 80)),
                (2, 'layers', (1, 4, 8)), (3, 'alpha', (15, 20, 30)),
                (5, 'spacing', (1, 4, 8)), (6, 'lifetime', (30, 60, 90)), (7, 'growth', (0, 1, 2))], phases=((1,),))),
    ((63,), Emitter([(0, 'palette', PALETTES), (1, 'size', (30, 60, 80)),
                (2, 'layers', (1, 4, 8)), (3, 'alpha', (15, 20, 30)),
                (5, 'spacing', (1, 4, 8)), (6, 'lifetime', (30, 60, 90)), (7, 'growth', (0, 1, 2))], phases=((1,),), tail=0)),
    ((24,), Emitter([(0, 'count', (1, 12, 24)), (1, 'batch', (1, 2, 4)),
             (2, 'width', (1, 15, 30)), (3, 'height', (1, 15, 30)),
             (4, 'height_step', (0, 16, 32)), (5, 'size', SIZES), (6, 'size_variation', (1, 5, 10)),
             (7, 'alpha', (32, 80, 160)), (9, 'fade', (-4, -8, -16))], speeds=(50, 100, 200), tail=56, depth=6)),
    ((26,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'interval', (3, 5, 10)), (2, 'radius', (30, 60, 100)),
             (3, 'width', SIZES), (4, 'width_variation', (1, 8, 16)),
             (5, 'height', (100, 200, 300)), (6, 'height_variation', (1, 100, 200)),
             (7, 'tilt', (0, 10, 25)), (8, 'cluster', (1, 3, 5))], phases=((2, 0),), tail=82, depth=6)),
    ((27,), Emitter([(0, 'palette', PALETTES), (1, 'size', (300, 500, 900)), (2, 'growth', (-5, -10, -30)),
             (3, 'radius', (200, 500, 800)), (4, 'spread', (10, 40, 80))], depth=6)),
    ((28,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'offset', (-25, 0, 25))], tail=63)),
    ((31,), Emitter([(0, 'palette', PALETTES), (1, 'radius', (80, 150, 250)), (2, 'lifetime', (30, 60, 90)),
             (3, 'interval', INTERVALS), (4, 'angular_step', (1, 5, 10)),
             (5, 'width', SIZES), (6, 'height', SIZES), (7, 'alpha', ALPHAS),
             (8, 'fade', (-1, -8, -16)), (9, 'growth', (1, 5, 10))], speeds=(-1, -2, -4), phases=((2,),), tail=0)),
    ((33,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'radius', (0, 15, 30)), (2, 'size', SIZES),
             (3, 'alpha_sixteenths', (1024, 2048, 4080)), (4, 'fade_sixteenths', (-16, -64, -128))], tail=61)),
    ((34,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'lifetime', (30, 60, 90)), (2, 'count', (1, 6, 12)),
             (3, 'width', SIZES), (4, 'width_variation', (1, 4, 8)),
             (5, 'height', SIZES), (6, 'height_variation', (1, 4, 8))], tail=0, fixed_palette=True)),
    ((36,), Emitter([(0, 'palette', PALETTES), (1, 'spread', (1, 15, 30)), (2, 'size', SIZES),
             (3, 'lifetime', LIFETIMES), (7, 'size_variation', (0, 1, 8))], speeds=(50, 100, 200), tail=0)),
    ((38,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'radius', (20, 80, 120)), (2, 'count', (1, 2, 6, 12)),
             (3, 'curvature', (0, 100, 300)), (4, 'size', SIZES),
             (8, 'cleanup', (0, 1)), (9, 'blend', (0, 1))], speeds=(0, 5, 10, 20), phases=((2,),), tail=121, depth=10, refracts=True)),
    ((46, 47), Emitter([(0, 'palette', PALETTES), (1, 'size', SIZES), (2, 'burst_size', SIZES),
                (3, 'fade', (-4, -10, -20))], speeds=(5, 10, 20), tail=61)),
    ((48,), Emitter([(0, 'palette', PALETTES), (1, 'radius', (30, 60, 120)),
             (2, 'width', SIZES), (3, 'width_variation', (1, 8)),
             (4, 'height', SIZES), (5, 'height_variation', (1, 8)), (6, 'alpha', ALPHAS),
             (7, 'rise', (100, 200, 400)), (8, 'interval', (0, 1, 2, 4)), (9, 'lifetime', (30, 60, 90))], tail=0)),
    ((49,), Emitter([(0, 'palette', PALETTES), (1, 'glow_size', SIZES),
             (2, 'spark_size', SIZES), (3, 'spark_lifetime', LIFETIMES)], phases=((1, 3),), depth=6)),
    ((51,), Emitter([(0, 'size', (30, 60, 100)), (1, 'duration', (15, 40, 78))], tail=59)),
    ((54,), Emitter([(0, 'palette', PALETTES + EMPTY_PALETTES), (1, 'radius', (0, 15, 30)), (2, 'size', SIZES),
             (3, 'size_variation', (0, 1, 8)),
             (5, 'opacity_and_speed_variation', (32, 128, 200, 255)), (6, 'fades', (0, 1)),
             (7, 'lifetime', LIFETIMES), (8, 'interval', INTERVALS)], speeds=(50, 100, 200), phases=((2, 0),), tail=0, fixed_palette=True)),
    ((55,), Emitter([(0, 'palette', PALETTES + RANDOM_PALETTES), (1, 'offset', (-20, 0, 20)), (2, 'lifetime', LIFETIMES),
             (3, 'size', (16, 32)), (4, 'alpha', ALPHAS), (5, 'fades', (0, 1)),
             (6, 'growth', (0, 1, 2)), (7, 'interval', INTERVALS), (8, 'orientation', (0, 1))], phases=((2, 0),), tail=0)),
    ((60,), Emitter([(0, 'count', (1, 2, 4))], tail=181, depth=10)),
    ((66,), Emitter([(0, 'curvature', (0, 50, 100, 200)), (1, 'size', (20, 30, 50)),
             (3, 'fade_sixteenths', (-64, -200, -400)), (7, 'launch_x', (-100, 100)),
             (8, 'launch_y', (-100, 0, 100)), (9, 'launch_z', (-100, 0, 100))], speeds=(5, 10, 20), tail=61)),
] for kind in kinds}


def properties(effect):
    kind = effect['kind']
    if kind == 'sprite':
        return ({'growth': SPRITE_PROPERTIES['growth']} if effect['variant'] in (27, 28)
                else SPRITE_PROPERTIES)
    return MODEL_PROPERTIES if kind == 'model' else ACTOR_PROPERTIES


def domains(effect):
    if effect['kind'] == 'emitter':
        definition = EMITTERS[effect['variant']]
        return {**{name: visual_values(effect['variant'], name, values) for name, values in definition.fields.items()},
                'movement_speed': definition.speeds}
    return {'lifetime': LIFETIMES, 'size': SIZES, 'alpha': ALPHAS, 'fade': FADES,
            'heading': (0, 15, 30, 210, 225, 240), 'speed': SPEEDS,
            **{name: values for name, (_, values) in properties(effect).items()}}


# Complete effect inputs are the unit of composition and shrinking. Numeric
# command slots are confined to this compiler, shared by the two test runners.
SPRITES = (0, 1, 2, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 18, 21, 22, 23, 25, 27, 28,
           40, 41, 42, 43, 49, 52, 53, 54, 69, 70, 74, 80, 501, 502, 503, 504, 505)
RESOURCE_EMITTERS = (40, 47, 50, 66, 70)
FIELD_TYPES = ([('sprite', n) for n in SPRITES] + [('model', n) for n in range(3)]
               + [('emitter', n) for n in EMITTERS if n not in RESOURCE_EMITTERS]
               + [('emote', n) for n in range(20)])
EMPTY_TAIL_UPDATES = 6
CLEANUP_CASES = ('emitter-38-cleanup-0-to-1', 'emitter-38-cleanup-1-to-0')
FAR_CLIP_DISTANCE = 40000
BACKDROP_SEPARATION = 2


def visual_values(variant, name, values, baseline=False):
    if baseline and name == 'palette':
        return PALETTES
    # A zero random range can span the full random sample and leave the stage.
    # Constructors, updates and shrinking must all keep visual cases in bounds.
    if ((baseline and name == 'interval') or name.endswith('variation')
            or (variant in (30, 75) and name == 'radius')):
        return tuple(value for value in values if value != 0)
    return values


def last_particle_edit(effect):
    """Keep writes on a live particle, before its storage can be reused."""
    p = effect['parameters']
    age = p['lifetime'] - 1
    linear = effect['kind'] == 'sprite' or (p.get('gravity_and_fade', 0) & 10) == 2
    if linear and p['fade'] < 0:
        age = min(age, (p['alpha'] - 1) // -p['fade'])
    if effect['kind'] == 'sprite' and effect['variant'] == 1:
        age = min(age, SMOKE_UPDATES - 1)
    if effect['kind'] == 'model':
        for axis in 'xyz':
            if (growth := p.get('growth_' + axis, 0)) < 0:
                age = min(age, (p['size'] - 1) // -growth)
    return effect['birth'] + age


def effect(kind, variant, camera, rng, index=0, actor=None, *, baseline=False):
    depth = EMITTERS[variant].depth if kind == 'emitter' else 2
    x, y = rng.randint(290, 350), rng.randint(210, 250)
    # Falling streams need room for their entire descent, including the drain
    # after removal. Start in the upper third of the 400-pixel comparison stage.
    if kind == 'emitter' and EMITTERS[variant].falling:
        y -= 100
    birth = rng.randint(0, 15)
    # Give visual comparisons time to show their subject. Very short lifetimes
    # remain shrink targets; their timing is also covered by simulation tests.
    lifetime = rng.choice((30, 60, 90))
    if kind == 'model':
        # Leave 140 pixels of travel around the middle of the stage. The domains
        # allow at most four world units of speed and one of acceleration.
        # Size scales with depth below, so distant cases remain observable.
        _, units = plane(camera, x, y)
        travel = 4*lifetime + lifetime*lifetime/2
        depth = max(depth, math.ceil(travel/(140*units)))
        y = 200
    # Leave room behind the subject for a visible checkerboard.
    maximum_depth = math.floor(FAR_CLIP_DISTANCE / math.dist(camera['position'], camera['target'])) - BACKDROP_SEPARATION
    if maximum_depth < 2:
        raise ValueError('camera leaves no room for the effect comparison stage')
    depth = min(depth, maximum_depth)
    position, units = plane(camera, x, y, depth)
    position = [round(v) for v in position]
    if kind == 'emitter':
        handle = 5000 + index if actor is None else actor
        parameters = {}
        for name, values in EMITTERS[variant].fields.items():
            values = visual_values(variant, name, values, baseline)
            if baseline and (name in ('size', 'width', 'height', 'glow_size', 'spark_size',
                                     'alpha', 'alpha_sixteenths')
                             or name.endswith('lifetime')):
                parameters[name] = max(values)
            else:
                parameters[name] = rng.choice(values)
        if variant in (10, 18, 19, 29, 32, 36, 37, 40, 46, 47, 50, 51, 56, 58, 61, 65, 66, 67, 68):
            parameters['target'] = [position[0]+rng.randint(50, 150), *position[1:]]
        if variant in (65, 68):
            # A vertical launch degenerates the expanding orbit's horizontal basis.
            parameters['direction_target'] = [position[0]+100, position[1], position[2]+200]
        speed = rng.choice(EMITTERS[variant].speeds)
        changes = []
        # Travelling motes can finish quickly; exercise restart during flight.
        change = birth + (1 if variant == 18 else rng.randint(10, 30))
        if fields := EMITTERS[variant].fields:
            name, values = rng.choice(list(fields.items()))
            values = visual_values(variant, name, values, baseline)
            changes.append([change, {name: rng.choice(values)}])
        transitions = EMITTERS[variant].phases
        for phase in transitions[0] if len(transitions) == 1 else rng.choice(transitions):
            changes.append([change, {'phase': phase}])
            change += rng.randint(10, 30)
        removal = max(change + rng.randint(20, 60), birth + EMITTERS[variant].minimum_run)
        if not baseline and fields:
            # Settings can change while an emitter's already-born particles drain.
            name, values = rng.choice(list(fields.items()))
            changes.append([rng.randint(change, removal-1),
                            {name: rng.choice(visual_values(variant, name, values))}])
        if not baseline and 'target' in parameters and variant != 67 and rng.choice((False, True)):
            # Moving bursts accept per-update displacement after launch.
            motion = ({'velocity': [rng.randint(0, 4), 0, 0]} if variant in (19, 32, 46, 47, 50, 51, 58) else
                      {'target': [position[0]+rng.randint(50, 150), *position[1:]]})
            changes.append([birth + rng.randint(1, 8),
                            motion])
        if variant == 18:
            # A restart after the first update still needs an outward and return
            # step. Shorter flights divide by zero in the source simulation.
            for values in [parameters] + [values for _, values in changes]:
                if 'target' in values:
                    values['target'][0] = max(values['target'][0], position[0] + 3*speed)
        if not baseline and variant == 22 and rng.choice((False, True)):
            removal = birth + rng.choice((1, 19, 20, 40, 100))
        return dict(kind=kind, variant=variant, handle=handle, birth=birth, position=position,
                    speed=speed, parameters=parameters, changes=changes, removal=removal,
                    **({'depth': depth} if depth != EMITTERS[variant].depth else {}))
    elif kind == 'sprite':
        refracts = variant in (27, 28)
        if refracts:
            # Distortion must cross a checker edge to be observable.
            position = [round(v) for v in plane(camera, 280, 200, 2)[0]]
        velocity = [rng.choice(VELOCITIES) for _ in range(3)]
        speed = rng.choice(SPEEDS) if sprite_directed(variant) else 0
        size = round(rng.choice(REFRACTION_PIXELS if refracts else SPRITE_PIXELS) * units)
        if variant == 22:
            size *= 3  # The halo's thin artwork needs enough visible pixels to compare.
        parameters = dict(lifetime=lifetime, velocity=velocity, speed=speed, size=size,
                          alpha=rng.choice(VISIBLE_ALPHAS), fade=rng.choice(FADES), palette=rng.choice(PALETTES))
    elif kind == 'model':
        parameters = dict(lifetime=lifetime, rotation=[rng.randrange(360) for _ in range(3)],
                          size=round(rng.choice(SPRITE_PIXELS) * units),
                          alpha=rng.choice(VISIBLE_ALPHAS), fade=rng.choice(FADES))
        # Independent axes exercise translation, rotation, growth and tint
        # without exhausting the fixture's command space in mixed scenarios.
        fields = [prefix+axes[rng.randrange(3)] for prefix, axes in
                  [('velocity_', 'xyz'), ('spin_', 'xyz'), ('growth_', 'xyz'), ('tint_', 'rgb')]]
        for name in fields + ['speed', 'directed', 'blend', 'gravity_and_fade']:
            parameters[name] = variant if name == 'blend' else rng.choice(MODEL_PROPERTIES[name][1])
    elif kind == 'emote':
        parameters = dict(lifetime=lifetime)
    else:
        raise ValueError(f'unknown effect kind {kind}')
    handle = -100-index if kind == 'emote' else 0x1f00+index*4
    result = dict(kind=kind, variant=variant, handle=handle, birth=birth, position=position, depth=depth,
                  parameters=parameters, changes=[])
    if not baseline and kind in ('sprite', 'model'):
        name, (_, values) = rng.choice(list(properties(result).items()))
        result['changes'].append([rng.randint(birth+1, last_particle_edit(result)), {name: rng.choice(values)}])
    return result


def composed_case(name, effects, seed):
    return dict(name=name, seed=seed, warmup=30, effects=effects, expires=True, contained=True)


def frame_ascent(case, camera):
    """Watch ascending discs from below so their flight stays inside the stage."""
    if not any(e['kind'] == 'emitter' and e['variant'] in (21, 23) for e in case['effects']):
        return case
    def basis(view):
        eye, target = (np.asarray(view[k], dtype=float) for k in ('position', 'target'))
        forward = target-eye
        distance = np.linalg.norm(forward)
        forward /= distance
        right = np.cross(forward, [0, 0, 1]); right /= np.linalg.norm(right)
        return eye, distance, np.column_stack((right, np.cross(right, forward), forward))
    eye, distance, axes = basis(camera)
    if axes[2, 2] > .99:
        return case
    angle = math.radians(3)
    target = np.asarray(camera['target'], dtype=float)
    view = dict(position=[round(v) for v in target-distance*np.array([0, math.sin(angle), math.cos(angle)])],
                target=[round(v) for v in target])
    new_eye, _, new_axes = basis(view)
    for e in case['effects']:
        if e['kind'] not in ('sprite', 'model', 'emitter'):
            continue
        before = np.asarray(e['position'])
        e['position'] = [round(v) for v in new_eye + new_axes @ axes.T @ (before-eye)]
        offset = np.asarray(e['position'])-before
        for values in [e['parameters']] + [values for _, values in e['changes']]:
            for name in ('target', 'direction_target'):
                if name in values:
                    values[name] = [round(v) for v in np.asarray(values[name])+offset]
    case['camera'] = view
    return case


def effect_setup(effect):
    kind, variant = effect['kind'], effect['variant']
    if kind == 'ring':
        return ring_setup(effect)
    if kind == 'model' or (kind == 'emitter' and variant == 50):
        return [[0x98, [68610]], [0x64, [1, None]], ['copy', [effect['handle']+0x40, 0x20]]]
    if kind == 'actor':
        # Keep the model handle until this actor's birth, even when other layers load resources.
        return [[0x98, [variant]], [0x64, [1, None]], ['copy', [0x1e00, 0x20]]]
    if (kind == 'emitter' and variant in (40, 47, 66, 70)) or (kind == 'sprite' and 32 <= variant < 40):
        bindings = [(variant-32 if kind == 'sprite' else 0, 68611)]
        if variant == 70:
            bindings.append((1, 68614))
        return [command for slot, resource in bindings for command in
                [[0x98, [resource]], [0x64, [1, None]], [0xd2, [slot, None, 0]]]]
    return []


def sprite_directed(variant):
    return variant not in (4, 21, 22, 25, 70) and not 501 <= variant <= 505


def emitter_vector_slot(variant, name):
    return 7 if name == 'target' and variant in (29, 65, 68) else 4


def effect_events(effect):
    """Compile named inputs; only this boundary knows command/property numbers."""
    kind, variant = effect['kind'], effect['variant']
    handle, birth, p = effect['handle'], effect['birth'], effect['parameters']
    if kind == 'emitter':
        definition = EMITTERS[variant]
        parameters = [0]*10
        for name, value in p.items():
            if name in ('target', 'direction_target'):
                slot = emitter_vector_slot(variant, name)
                parameters[slot:slot+3] = value
            else:
                parameters[definition.slots[name]] = value
        speed = effect['speed']
        resource = {'variable': handle+0x40} if variant == 50 else 0
        yield [birth, 0xbf, [handle, *effect['position'], resource, variant, 0, speed, *parameters]]
        yield [birth, 0x1d, [handle, 34, 3]]
        yield [birth, 0x1d, [handle, 5, speed]]
    elif kind in ('sprite', 'model'):
        directed = kind == 'sprite' and sprite_directed(variant)
        args = [variant if kind == 'sprite' else {'variable': handle+0x40}, p['lifetime'],
                *effect['position'], *p['velocity' if kind == 'sprite' else 'rotation']]
        args += [p['speed']] if directed else []
        args += [p['size'], p['alpha'], p['fade']]
        args += [p['palette'], 0] if kind == 'sprite' else []
        yield [birth, 0xdd if kind == 'model' else 0xd3 if directed else 0xd0, args]
        yield [birth, 'copy', [handle, 0x20]]
        if kind == 'model':
            for name, value in p.items():
                if name in MODEL_PROPERTIES:
                    yield [birth, 0xde, [{'variable': handle}, MODEL_PROPERTIES[name][0], value]]
    elif kind == 'emote':
        yield [birth, 0x10, [handle, 0, 0, 0, variant, 1, 0, p['lifetime']]]
    elif kind == 'actor':
        yield [birth, 0x10, [handle, *effect['position'], 0, {'variable': 0x1e00}, 0, 0]]
    elif kind == 'station':
        yield [birth, 0x5c, [handle, *effect['position'], 12, variant]]
        yield [birth, 0xb2, [handle, 0]]
        if p.get('transfer'):
            yield [birth, 0x20, [55, 1]]
            yield [birth, 0x19, [1, 0, 150, 0]]
            yield [birth, 0x14, [1, 0]]
            yield [birth, 0x5d, [0]]
    elif kind == 'ring':
        mode, variation = variant
        for op, args in [[0x20, [55, 1]], [0x78, [mode, 0 if mode == 10 else variation]], [0x5d, [0]]]:
            yield [birth, op, args]
        if mode == 12:
            yield [birth+70, 0x1d, [1, 10, 1]]
            yield [birth+70, 0x19, [1, 4000, 0, 0]]
    else:
        raise ValueError(f'unknown effect kind {kind}')
    for tick, changes in effect['changes']:
        for name, value in changes.items():
            if kind == 'emitter':
                if name in ('target', 'velocity', 'direction_target'):
                    for axis, coordinate in enumerate(value):
                        yield [tick, 0x1d, [handle, 113+emitter_vector_slot(variant, name)+axis, coordinate]]
                else:
                    property = 33 if name == 'phase' else 5 if name == 'movement_speed' else 113+definition.slots[name]
                    yield [tick, 0x1d, [handle, property, value]]
            elif name == 'heading':
                yield [tick, 0x14, [handle, value]]
            else:
                op = {'sprite': 0xd1, 'model': 0xde, 'actor': 0x1d, 'station': 0x1d}[kind]
                target = handle if kind in ('actor', 'station') else {'variable': handle}
                yield [tick, op, [target, properties(effect)[name][0], value]]
    if 'removal' in effect:
        yield [effect['removal'], 0x12, [handle]]
        if kind == 'emitter' and variant == 22:
            # Camera shake is global; interrupted quakes need explicit cleanup.
            yield [effect['removal'], 0x4a, [0, 0, 0]]


def capture_settings(case):
    """Derive capture requirements after generation or shrinking changes inputs."""
    settings = dict(case)
    effects = case.get('effects', [])
    if effects and not any(e['kind'] == 'ring' for e in effects):
        settings['updates'] = 0
    visible = False
    for e in effects:
        if e['kind'] == 'emitter':
            settings_over_time = [e['parameters']] + [values for _, values in e['changes']]
            tail = max([EMITTERS[e['variant']].tail] + [max(0, value)+1
                       for values in settings_over_time for name, value in values.items()
                       if name.endswith('lifetime')])
            end = e['removal'] + tail
        elif e['kind'] in ('actor', 'station'):
            # Wing sparks can finish after their model is removed.
            end = e['removal'] + 21
        else:
            end = e['birth'] + e['parameters'].get('lifetime', 0)
        # Include the birth and final presentations before checking an empty tail.
        settings['updates'] = max(settings.get('updates', 0), case['warmup'] + end + 2 + EMPTY_TAIL_UPDATES)
        if e['kind'] == 'emote':
            visible |= e['variant'] not in INVISIBLE_EMOTES
        elif e['kind'] == 'emitter' and EMITTERS[e['variant']].fixed_palette:
            colors = [values['palette'] for values in
                      (settings_over_time[:1] if e['variant'] in (16, 34) else settings_over_time) if 'palette' in values]
            visible |= any(color not in EMPTY_PALETTES for color in colors)
        elif e['kind'] == 'emitter' and e['variant'] == 48:
            visible |= any(values.get('interval', 0) != 0 for values in settings_over_time)
        else:
            visible = True
        settings['checkerboard'] = (settings.get('checkerboard', False) or e['kind'] == 'model'
            or (e['kind'] == 'sprite' and (e['variant'] in (27, 28)
                or any('blend' in values for _, values in e['changes'])))
            or (e['kind'] == 'emitter' and EMITTERS[e['variant']].refracts))
    settings['backdrop_depth'] = max([2] + [e.get('depth', EMITTERS[e['variant']].depth if e['kind'] == 'emitter' else 2)
                                         for e in effects]) + BACKDROP_SEPARATION
    settings.setdefault('visible', visible if case.get('effects') else True)
    return settings


def cases(camera, family='field'):
    if family == 'tower':
        yield from tower_cases(camera)
    elif family == 'rings':
        yield from ring_cases(camera)
    elif family == 'stations':
        station = {'name': 'station', 'warmup': 30, 'visible': True, 'expires': True,
               'contained': True, 'no_matte': True,
               'camera': {'position': [700, -324, 736], 'target': [0, 97, 87]},
               'effects': [dict(kind='station', variant=363, handle=5000, birth=0,
                                position=[0, 100, -60], parameters={}, changes=[], removal=80)]}
        yield station
        transfer = copy.deepcopy(station)
        transfer.update(name='station-transfer', interactions=[5000],
                        camera={'position': [1400, -745, 1385], 'target': [0, 97, 87]},
                        inputs=[{'poll': 100, 'duration': 10, 'buttons': ['a']}])
        transfer['effects'][0].update(parameters={'transfer': True}, removal=140)
        yield transfer
    elif family == 'field':
        for kind, variant in FIELD_TYPES:
            name = f'{kind}-{variant}'
            yield frame_ascent(composed_case(name, [effect(kind, variant, camera, random.Random(name), baseline=True)], 1), camera)
        for before, name in enumerate(CLEANUP_CASES):
            subject = effect('emitter', 38, camera, random.Random(name), baseline=True)
            subject['parameters']['cleanup'] = before
            subject['changes'] = [[tick, values] for tick, values in subject['changes'] if 'cleanup' not in values]
            release = next(tick for tick, values in subject['changes'] if values.get('phase') == 2)
            subject['changes'].append([release+1, {'cleanup': 1-before}])
            yield composed_case(name, [subject], 1)
    else:
        raise ValueError(f'unknown effect family {family}')


def pin_camera(view):
    commands = [[0xa8, [*view['position'], *view['target'], 1, 1]]]
    # The positioning command leaves zero coordinates unchanged.
    for axis, value in enumerate(view['position'] + view['target']):
        if value == 0:
            commands += [[0xa1, [8+axis*2+bound, 0]] for bound in range(2)]
    return commands


RING_NAMES = ['fire', 'fire-alias', 'shrink', 'mana', 'electric-sylvarant', 'radar',
             'water', 'wind', 'long-range-fire', 'sunlight', 'electric-tethealla', 'bomb',
             'lightning', 'ice', 'earthquake', 'darkness', 'sound', 'animal-call', 'bubble']

TOWER_MODELS = [('colette-wings', 90021, 131428, 535),
                                         ('wing-slot-90022', 90022, 131432, 534),
                                         ('wing-slot-90023', 90023, 131439, 150),
                                         ('kratos-wings', 90024, 131430, 535),
                                         ('wing-slot-90025', 90025, 131437, 416),
                                         ('yggdrasil-wings', 90026, 131433, 535),
                                         ('aura-131435', 90027, 131435, 535),
                                         ('aura-131436', 90028, 131436, 535),
                                         ('eternal-sword', 1015, 131441, 535),
                                         ('eternal-sword-halo', 1016, 131442, 535)]

def case_names(family):
    if family == 'field':
        return [f'{kind}-{variant}' for kind, variant in FIELD_TYPES] + list(CLEANUP_CASES)
    if family == 'tower':
        return ([f'bound-sprite-{slot}' for slot in range(8)]
                + [f'emitter-{n}' for n in RESOURCE_EMITTERS if n != 66]
                + ['renegade-shot'] + [row[0] for row in TOWER_MODELS])
    if family == 'rings':
        return [f'ring-{name}-{variant}' for mode, name in enumerate(RING_NAMES, 1) for variant in ring_variants(mode)]
    if family == 'stations':
        return ['station', 'station-transfer']
    raise ValueError(f'unknown effect family {family}')


def ring_variants(mode):
    return range(3 if mode in (13, 18) else 2 if mode in (10, 19) else 1)


def ring_setup(effect):
    mode, variant = effect['variant']
    commands = []
    if mode in (6, 16, 19):
        commands += [[0xac, [0, 0, 0, 0, 0, 0]]]
    if mode in (3, 12, 19):
        commands += [[0x1d, [1, prop, value]] for prop, value in [(38, 0), (40, 1), (11, 1), (12, 0)]]
    if mode == 10:
        commands += [[0xda, [300]]]
    if mode == 19 or (mode == 10 and variant == 0):
        commands += [[0x1d, [1, prop, 0]] for prop in (30, 31, 32)]
    if mode == 12:
        commands += [[0x1d, [1, 8, 0]], [0x1d, [1, 48, 1]]]
    return commands + [[0x14, [1, effect['parameters']['heading']]]]


def ring_cases(camera):
    for mode, name in enumerate(RING_NAMES, 1):
        for variant in ring_variants(mode):
            view = {key: [round(v) for v in values] for key, values in camera.items()}
            distance = 4 if mode == 12 else 2 if mode in [17, 18] else 1 if mode == 19 else 1.5
            view['position'] = [round(t + (p-t)*distance) for p, t in zip(camera['position'], camera['target'])]
            if mode == 12:
                # Keep the falling explosion particles inside the frame.
                view['position'][2] -= 700
                view['target'][2] -= 700
            framed = mode in [9, 12, 16, 17, 18, 19]
            yield {'name': f'ring-{name}-{variant}', 'warmup': 30, 'updates': 750 if mode == 6 else 400,
                   'visible': True,
                   'expires': mode in [1, 2, 4, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17, 18] or (mode == 19 and variant == 0),
                   'contained': True,
                   **({'camera': view} if framed else {}),
                   **({'checkerboard': True} if mode in [6, 15, 16, 19] else {}),
                   'effects': [dict(kind='ring', variant=[mode, variant], handle=1, birth=0,
                                    parameters=dict(heading=225), changes=[])]}


def tower_cases(camera):
    rng = random.Random(0)
    for variant in RESOURCE_EMITTERS:
        if variant != 66:
            name = f'emitter-{variant}'
            case = composed_case(name, [effect('emitter', variant, camera, random.Random(name), baseline=True)], 1)
            if variant == 70:
                case['map'] = 58  # This field supplies both cylinder textures.
            yield case
    for slot in range(8):
        yield composed_case(f'bound-sprite-{slot}', [effect('sprite', 32+slot, camera, rng)], 1)
    center = [round(value) for value in plane(camera, 320, 240, 1)[0]]
    shot_camera = {key: [round(v) for v in values] for key, values in camera.items()}
    shot_camera['position'] = [round(t + 2*(p-t)) for p, t in zip(shot_camera['position'], shot_camera['target'])]
    yield {'name': 'renegade-shot', 'warmup': 30, 'visible': True, 'expires': True,
           'contained': True, 'camera': shot_camera,
           'effects': [dict(kind='emitter', variant=66, handle=5000, birth=0, position=center, speed=8,
                            parameters=dict(size=50, fade_sixteenths=-400, target=[center[0]+200, *center[1:]],
                                            launch_x=100, launch_y=0, launch_z=0),
                            changes=[], removal=80)]}
    for name, actor, resource, map_id in TOWER_MODELS:
        framing = {}
        if name == 'wing-slot-90022':
            view = {key: [round(v) for v in values] for key, values in camera.items()}
            view['position'] = [round(t + 2*(p-t)) for p, t in zip(camera['position'], camera['target'])]
            framing = {'camera': view, 'contained': True}
        yield {'name': name, **({'map': map_id} if map_id != 535 else {}),
               **framing,
               'warmup': 30, 'visible': True, 'no_matte': True, 'contained': True,
               'effects': [dict(kind='actor', variant=resource, handle=actor, birth=0, position=center,
                                parameters={}, changes=[], removal=120)]}


def scenario_commands(case):
    """Compile independent effect timelines into one ordered scenario program."""
    if 'camera' in case:
        yield from pin_camera(case['camera'])
    for e in case['effects']:
        yield from effect_setup(e)
    yield from SYNC
    events = [event for e in case['effects'] for event in effect_events(e)]
    tick = 0
    # Stable sorting preserves constructor/property ordering at each tick.
    for at, action, values in sorted(events, key=lambda e: e[0]):
        if at > tick:
            yield [0x64, [0, at-tick]]
        yield [action, values]
        tick = at


def random_cases(camera, bases, count, seed):
    rng = random.Random(seed)
    subjects = list(bases)
    for index in range(count):
        if index % len(subjects) == 0:
            rng.shuffle(subjects)
        base = subjects[index % len(subjects)]
        view = base.get('camera', camera)
        case = copy.deepcopy(base)
        case.update(name=f'random-{seed}-{index:03}', seed=rng.randrange(1, 2**32))
        subject = case['effects'][0]
        kind, variant = subject['kind'], subject['variant']
        if kind == 'ring' or case.get('interactions'):
            # Batch a small set of controller timelines on resident workers.
            shift = rng.choice((-20, 0, 20))
            case['inputs'] = [dict(row, poll=row['poll']+shift) for row in base.get('inputs', [])]
        if kind == 'ring':
            subject['parameters']['heading'] = rng.choice((210, 225, 240))
        elif kind in ('actor', 'station'):
            subject['removal'] = rng.choice((120, 140, 160) if case.get('interactions') else (30, 60, 120))
            subject['changes'] = [[0, dict(heading=rng.choice((0, 15, 30)),
                                          **{name: rng.choice(values) for name, (_, values) in ACTOR_PROPERTIES.items()})]]
        else:
            subject = effect(kind, variant, view, rng)
        choices = (FIELD_TYPES if kind not in ('ring', 'actor', 'station') else
                   [entry for entry in FIELD_TYPES if entry[0] in ('emitter', 'sprite')])
        first_actor = max(4999, subject['handle']) + 1
        reuse = kind == 'emitter' and rng.choice((False, True))
        case['effects'] = [subject] + [effect(*rng.choice(choices), view, rng, i+1, first_actor+i)
                                      for i in range(rng.randrange(2 if reuse else 3))]
        if reuse:
            replacement = effect('emitter', rng.choice([v for k, v in choices if k == 'emitter']), view, rng,
                                 len(case['effects']), subject['handle'])
            delay = subject['removal'] + rng.choice((0, 1, 2)) - replacement['birth']
            replacement['birth'] += delay
            replacement['removal'] += delay
            replacement['changes'] = [[tick+delay, values] for tick, values in replacement['changes']]
            case['effects'].append(replacement)
        yield frame_ascent(case, view)


def minimize(case, fails):
    """Reduce independent inputs while preserving a failing, valid scenario."""
    case = copy.deepcopy(case)
    def get(path, root=None):
        value = case if root is None else root
        for key in path:
            value = value[key]
        return value
    def attempt(path, value=None, *, remove=False):
        nonlocal case
        candidate = copy.deepcopy(case)
        parent = get(path[:-1], candidate)
        if remove:
            del parent[path[-1]]
        else:
            parent[path[-1]] = value
        if any(not e['birth'] <= tick <= last_particle_edit(e)
               for e in candidate['effects'] if e['kind'] in ('sprite', 'model')
               for tick, _ in e['changes']):
            return False
        if fails(candidate):
            case = candidate
            return True
        return False
    index = 0
    while index < len(case.get('effects', [])) and len(case['effects']) > 1:
        if not attempt(('effects', index), remove=True):
            index += 1
    for layer in range(len(case.get('effects', []))):
        root = ('effects', layer)
        emitter = get(root)['kind'] == 'emitter'
        timeline = root + ('changes',)
        get(timeline).sort(key=lambda event: event[0])
        index = 0
        while index < len(get(timeline)):
            if not attempt(timeline+(index,), remove=True):
                index += 1
        effect = get(root)
        inputs = domains(effect)
        fields = [(root+('parameters', name), inputs[name]) for name in effect['parameters'] if name in inputs]
        if emitter:
            fields += [(root+('speed',), EMITTERS[effect['variant']].speeds)]
        fields += [(timeline+(i, 1, name), inputs[name]) for i, (_, changes) in enumerate(effect['changes'])
                   for name in changes if name in inputs]
        for path, values in fields:
            for value in sorted(set(values), key=lambda v: (abs(v), v)):
                if abs(value) < abs(get(path)):
                    attempt(path, value)
        times = [root+('birth',)] + [timeline+(i, 0) for i in range(len(get(timeline)))]
        if 'removal' in effect:
            times += [root+('removal',)]
        earliest = max([0] + [other['removal'] for other in case['effects']
                             if emitter and other['kind'] == 'emitter'
                             and other['handle'] == effect['handle'] and other['birth'] < effect['birth']])
        previous = None
        for path in times:
            for tick in (0, 1, 2, 8, 30, 60):
                if (get(previous) if previous else earliest) <= tick < get(path):
                    attempt(path, tick)
            previous = path
    for index in range(len(case.get('inputs', []))):
        for field, values in [('poll', (case['warmup']*2,)), ('duration', (2, 4, 8))]:
            path = ('inputs', index, field)
            for value in values:
                if value < get(path):
                    attempt(path, value)
    return case
