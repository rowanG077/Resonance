"""Expand current compact effect settings for the observation calculations.

These are the authoring defaults of resonance_content::battle_effect. This does
not read an older layout; the containing library version is checked by Snapshot.
"""
import copy

TEMPLATE = dict(lifetime=None, follow_origin=False, follow_orientation=False,
    draw_after_target=False, element_tint=False, jerk=[0,0,0], orbit_velocity=[0,0,0],
    geometry_acceleration_until=None, gradient=False, fade=[0,0,0,0], fade_from=0,
    uv_animation=None)
STATE = dict(cull_back=False, offset=[0,0,0], velocity=[0,0,0], acceleration=[0,0,0],
    angles=[0,0,0], angular_velocity=[0,0,0], orbit=[0,0,0], brighten=[0,0,0,0],
    brighten_until=None, uv=[0,0,0,0], palettes=[0,0], geometry_count=0)
GEOMETRY = dict(
    BillboardTrail=dict(radius_velocity=0, segment_size_step=[0,0,0], segment_offset=[0,0,0], segment_angle_step=0),
    Ribbon=dict(jitter=0,phase=0,phase_period=0),
    Spiral=dict(velocity=[0,0,0],acceleration=[0,0,0],segment_angle_step=0),
    Size=dict(velocity=[0,0,0],acceleration=[0,0,0]),
    Quad=dict(velocity=[[0,0,0] for _ in range(4)]))
PARTICLE_VISUAL = dict(depth_test=True,depth_write=False,ground_relative=False,
    anchored=False,copies=0,copy_rotation=0,after_actor=False)
MODEL_VISUAL = dict(depth_test=True,lit=False,cull_back=False,before_actor=False)
SHAPE = dict(ring=dict(flared=False,hidden=False), radial_quads=dict(flat=False,advance_u=False),
    ribbon=dict(depth=False),spiral=dict(radius_step=0,segment_offset=[0,0,0]),
    shell=dict(elliptical=False),vertex_quad=dict(copy_axis=0,local_copies=False))
BINDING = dict(animation=None, repeat=False)
MODEL_ANIMATION = dict(blend_ticks=0,rate=1,repeat=False)
SPREAD = dict(angles=[0,0,0],radius=0,radius_jitter=0,angle_jitter=0)
MOTION = {key:[None,None,None] for key in ('offset','size','velocity','acceleration',
    'angular_velocity','size_velocity','size_acceleration')}
RANGE_FIELDS = set(('offset velocity acceleration angles angular_velocity segment_offset orbit '
    'orbit_velocity size size_velocity size_acceleration vertices color end_color uv brighten fade '
    'segment_angle_step brighten_until lifetime fade_from geometry_count phase palette alpha_palette model').split())


def defaults(value, settings):
    for key, setting in settings.items():
        value.setdefault(key, copy.deepcopy(setting))


def ranges(value):
    if isinstance(value, (int,float)) and not isinstance(value,bool):
        return dict(min=value,max=value)
    if isinstance(value,list):
        return [ranges(item) for item in value]
    return value


def birth(value):
    for key in RANGE_FIELDS & value.keys():
        value[key] = ranges(value[key])
    for total in value.get('sums',[]):
        total['terms'] = [ranges(term) for term in total['terms']]
    if value.get('animation') is not None:
        defaults(value['animation'], MODEL_ANIMATION)
    for key in ('offset_spread','velocity_spread'):
        if value.get(key) is not None:
            defaults(value[key],SPREAD)


def programs(values, names):
    """Expand named repetitions and local references into the runtime schedule."""
    def samples(value, base, end):
        if isinstance(value, list):
            for item in value:
                samples(item, base, end)
        elif isinstance(value, dict):
            for key, item in value.items():
                if key in ('draw', 'radius_draw', 'angle_draw') and item is not None:
                    absolute = base + item
                    if not 0 <= absolute < end:
                        raise ValueError("unbound effect random sample")
                    value[key] = absolute
                else:
                    samples(item, base, end)

    result = []
    for entries in values:
        events = []
        def append(event, base):
            repeat = event.get('repeat', 1)
            if not 0 < repeat <= 65536 - len(events):
                raise ValueError("invalid effect repeat")
            operation = {key: value for key, value in event.items() if key not in ('at','repeat','every')}
            for iteration in range(repeat):
                at = base + event.get('at', 0) + iteration * event.get('every', 0)
                if not 0 <= at <= 0xffffffff:
                    raise ValueError("effect time overflow")
                events.append(dict(at=at, operation=copy.deepcopy(operation)))
        for entry in entries:
            if 'events' in entry:
                repeat = entry['repeat']
                total = repeat * sum(event.get('repeat', 1) for event in entry['events'])
                if repeat <= 0 or not entry['events'] or not 0 < total <= 65536 - len(events):
                    raise ValueError("invalid effect group")
                for iteration in range(repeat):
                    for event in entry['events']:
                        append(event, entry.get('at', 0) + iteration * entry.get('every', 0))
            else:
                append(entry, 0)
        events.sort(key=lambda event: event['at'])
        draws = spawns = 0
        def previous(value):
            if not 0 < value <= spawns:
                raise ValueError("particle target precedes this program")
            return spawns - value
        for event in events:
            operation = event['operation']
            if operation['kind']=='spawn':
                operation['particle'] = names.index(operation['particle'])
                defaults(operation,dict(blend=None,palette=None,birth={}))
                value = operation['birth']
                end = draws + value.get('random_draws', 0)
                samples(value, draws, end)
                if value.get('model_parent') is not None:
                    parent = value['model_parent']
                    parent['spawn'] = previous(parent.pop('previous_spawn'))
                draws = end
                spawns += 1
                birth(value)
            elif operation['kind']=='sound':
                defaults(operation,dict(priority=0))
            elif operation['kind']=='set_motion':
                operation['spawn'] = previous(operation.pop('previous_spawn'))
                defaults(operation,MOTION)
                if operation.get('animation') is not None:
                    defaults(operation['animation'],MODEL_ANIMATION)
                if operation.get('forward_motion') is not None:
                    defaults(operation['forward_motion'],dict(velocity=None,acceleration=None))
        result.append(events)
    return result


def templates(values):
    values = copy.deepcopy(values)
    for entry in values:
        if entry['kind'] not in ('particle','model_particle'):
            continue
        template = entry['template'];defaults(template,TEMPLATE)
        state = template['state'];defaults(state,STATE)
        for kind,parameters in state['geometry'].items():
            defaults(parameters,GEOMETRY.get(kind,{}))
        uv = template.get('uv_animation')
        if uv is not None:
            defaults(uv,dict(loop_start=None) if uv['kind']=='frames' else dict(origin=[0,0],step=[0,0]))
        visual = entry['visual']
        if entry['kind']=='particle':
            defaults(visual,PARTICLE_VISUAL)
            defaults(visual['geometry'],SHAPE.get(visual['geometry']['kind'],{}))
            if visual['texture']['kind']=='atlas':
                defaults(visual['texture'],dict(dual=False,palette_set=0))
        else:
            defaults(visual,MODEL_VISUAL)
            defaults(entry['binding'],BINDING)
    return values
