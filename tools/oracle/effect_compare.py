"""Image gates and stage geometry shared by effect comparisons."""
import json
import math
from pathlib import Path
import subprocess

import numpy as np

# Shared visual gates tolerate rounding and raster fringes, while rejecting
# substantial compact disagreements even inside a much larger matching effect.
GATES = {'pixel_tolerance': 8, 'max_changed_fraction': 0.05,
         'max_mean_error': 3.0, 'max_local_error': 16.0,
         'max_interior_error': 16,
         'max_bounds_delta': 1, 'minimum_active_pixels': 16}
FIELD_OF_VIEW = 27


def write(path, data):
    path = Path(path)
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(data, indent=2, allow_nan=False) + '\n')
    temporary.replace(path)


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
    error = abs(a-r)
    window = min(16, w, h)
    integral = np.pad(error.sum(2).cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    regions = (integral[window:, window:] - integral[:-window, window:]
               - integral[window:, :-window] + integral[:-window, :-window])
    local_error = float(regions[::max(1, window//2), ::max(1, window//2)].max()) / (window*window*3)
    pixel_error = error.max(2)
    different = pixel_error > GATES['pixel_tolerance']
    def compact_error(pixels):
        return (int(np.minimum.reduce([pixels[:-1, :-1], pixels[1:, :-1],
                                       pixels[:-1, 1:], pixels[1:, 1:]]).max())
                if min(w, h) >= 2 else 0)
    interior_error = compact_error(pixel_error)
    if interior_error > GATES['max_interior_error']:
        # An allowed edge shift can form a solid error patch. Check both images
        # for colors missing from the other's immediate neighborhood.
        radius = GATES['max_bounds_delta']
        def unmatched(left, right):
            padded = np.pad(right, ((radius, radius), (radius, radius), (0, 0)), mode='edge')
            return np.minimum.reduce([abs(left-padded[dy:dy+h, dx:dx+w]).max(2)
                                      for dy in range(2*radius+1) for dx in range(2*radius+1)])
        interior_error = compact_error(np.maximum(unmatched(r, a), unmatched(a, r)))
    # Ignore tolerated rounding fringes when measuring geometry.
    rb, ab = bounds(rm & (am | different)), bounds(am & (rm | different))
    count = int(mask.sum())
    result = {'reference_active_pixels': int(rm.sum()), 'actual_active_pixels': int(am.sum()),
              'reference_bounds': bounds(rm), 'actual_bounds': bounds(am),
              # Normalize tiny effects using the visibility floor.
              'mean_error': (float(error[mask].sum()) / (3 * max(count, GATES['minimum_active_pixels']))
                             if count else float(error.mean())),
              'changed_fraction': float(different[mask].mean()) if count else float(different.mean()),
              'max_local_error': local_error,
              'max_interior_error': interior_error,
              'bounds_delta': max(abs(v-w) for v, w in zip(rb, ab)) if rb and ab else 0 if rb == ab else None}
    result['passed'] = (result['mean_error'] <= GATES['max_mean_error']
                        and result['changed_fraction'] <= GATES['max_changed_fraction']
                        and local_error <= GATES['max_local_error']
                        and interior_error <= GATES['max_interior_error']
                        and result['bounds_delta'] is not None
                        and result['bounds_delta'] <= GATES['max_bounds_delta'])
    return result


def validate_matte(image, color):
    pixels = np.asarray(image.convert('RGB'), dtype=np.int16)
    # Outside every probe and the bottom-right field prompt.
    samples = pixels[10:20, 10:630]
    if np.abs(samples-np.asarray(color)).max() > 1:
        raise ValueError('invalid fixture: the isolation matte is missing or contaminated')


def validate_checker(image):
    samples = np.asarray(image.convert('RGB'), dtype=np.int16)[80, 80:561:80]
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


def run_command(args, log):
    with Path(log).open('w') as f:
        subprocess.run(list(map(str, args)), stdout=f, stderr=subprocess.STDOUT, check=True)
