#!/usr/bin/env python3
"""Read-only background-music analysis. Synthesis/cooking remain in Rust.

Registers one stereo PCM window, then compares a continuous interval without
resampling, gain correction, filtering, or per-window realignment. Local lag
searches and mean-removed errors are diagnostics only, never replacement PCM.
Requires the numpy/scipy packages supplied by the development flake.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import wave

import numpy as np
from scipy import signal


def read_pcm(path):
    with wave.open(str(path), "rb") as wav:
        if (wav.getnchannels(), wav.getsampwidth(), wav.getcomptype()) != (2, 2, "NONE"):
            raise ValueError("requires uncompressed signed-16-bit stereo WAV")
        rate = wav.getframerate()
        pcm = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").reshape(-1, 2)
    return rate, pcm.astype(np.float64)


def alignment(reference, actual):
    """Return actual offset and centered stereo correlation, without fitting gain."""
    n = len(reference)
    if n < 2 or len(actual) < n:
        raise ValueError("registration exceeds available PCM")
    centered = reference - reference.mean(axis=0)
    energy = np.sum(centered * centered)
    if energy < 1:
        raise ValueError("cannot register silent reference")
    numerator = sum(signal.correlate(actual[:, c], centered[:, c], mode="valid", method="fft")
                    for c in range(2))
    sums = np.vstack([np.zeros((1, 2)), np.cumsum(actual, axis=0)])
    squares = np.vstack([np.zeros((1, 2)), np.cumsum(actual * actual, axis=0)])
    window_sum = sums[n:] - sums[:-n]
    window_energy = np.sum(squares[n:] - squares[:-n] - window_sum * window_sum / n, axis=1)
    denominator = np.sqrt(energy * np.maximum(window_energy, 0))
    coefficients = np.divide(numerator, denominator, out=np.full_like(numerator, -np.inf),
                             where=denominator > 0)
    offset = int(np.argmax(coefficients))
    return offset, float(coefficients[offset])


def rms(values):
    return float(np.sqrt(np.mean(values * values)))


def db_ratio(numerator, denominator):
    return 20 * math.log10(numerator / denominator) if numerator > 0 and denominator > 0 else None


def metrics(reference, actual):
    a = reference - reference.mean(axis=0)
    b = actual - actual.mean(axis=0)
    error = actual - reference
    centered_error = b - a
    norm = math.sqrt(float(np.sum(a * a) * np.sum(b * b)))
    return {
        "frames": len(reference),
        "correlation": float(np.sum(a * b) / norm) if norm else None,
        "level_difference_db": db_ratio(rms(b), rms(a)),
        "reference_rms_pcm16": rms(reference),
        "actual_rms_pcm16": rms(actual),
        "reference_channel_means": reference.mean(axis=0).tolist(),
        "actual_channel_means": actual.mean(axis=0).tolist(),
        "channel_level_difference_db": [db_ratio(rms(b[:, c]), rms(a[:, c])) for c in range(2)],
        "channel_correlations": [float(np.corrcoef(a[:, c], b[:, c])[0, 1])
                                 if rms(a[:, c]) > 0 and rms(b[:, c]) > 0 else None
                                 for c in range(2)],
        "raw_error_rms_pcm16": rms(error),
        "raw_error_max_pcm16": float(np.max(np.abs(error))),
        "raw_signal_to_error_db": db_ratio(rms(reference), rms(error)),
        "mean_removed_error_rms_pcm16": rms(centered_error),
        "mean_removed_signal_to_error_db": db_ratio(rms(a), rms(centered_error)),
        "changed_samples": int(np.count_nonzero(error)),
        "reference_clipped_samples": int(np.count_nonzero(np.abs(reference) >= 32767)),
        "actual_clipped_samples": int(np.count_nonzero(np.abs(actual) >= 32767)),
        "reference_side_to_mid_db": db_ratio(rms(a[:, 0] - a[:, 1]), rms(a[:, 0] + a[:, 1])),
        "actual_side_to_mid_db": db_ratio(rms(b[:, 0] - b[:, 1]), rms(b[:, 0] + b[:, 1])),
    }


def compare(reference, actual, rate, reference_start, search_start, search_end,
            registration_frames, frames, window_frames, lag_radius):
    if (reference_start < 0 or search_start < 0 or search_end > len(actual)
            or frames < registration_frames or reference_start + frames > len(reference)):
        raise ValueError("comparison exceeds available PCM")
    offset, coefficient = alignment(reference[reference_start:reference_start + registration_frames],
                                    actual[search_start:search_end])
    actual_start = search_start + offset
    if actual_start + frames > len(actual):
        raise ValueError("registered comparison exceeds actual recording")
    ref = reference[reference_start:reference_start + frames]
    act = actual[actual_start:actual_start + frames]
    windows = []
    for start in range(0, frames, window_frames):
        end = min(frames, start + window_frames)
        item = metrics(ref[start:end], act[start:end])
        item['seconds'] = start / rate
        first = max(0, actual_start + start - lag_radius)
        last = min(len(actual), actual_start + end + lag_radius)
        lag, match = alignment(ref[start:end], actual[first:last])
        item['diagnostic_best_lag_frames'] = first + lag - (actual_start + start)
        item['diagnostic_best_correlation'] = match
        windows.append(item)
    frequencies, ref_power = signal.welch(ref, fs=rate, nperseg=4096, axis=0)
    _, act_power = signal.welch(act, fs=rate, nperseg=4096, axis=0)
    bands = []
    edges = [20, 60, 125, 250, 500, 1000, 2000, 4000, 8000, rate / 2]
    for low, high in zip(edges, edges[1:]):
        selected = (frequencies >= low) & (frequencies < high)
        ar = float(np.sum(ref_power[selected])); br = float(np.sum(act_power[selected]))
        bands.append({'hz': [low, high], 'actual_minus_reference_db': db_ratio(math.sqrt(br), math.sqrt(ar))})
    return {
        'sample_rate': rate,
        'registration': {'reference_start_frame': reference_start, 'actual_start_frame': actual_start,
                         'frames': registration_frames, 'correlation': coefficient,
                         'searched_actual_frames': [search_start, search_end]},
        'compared_seconds': frames / rate,
        'summary': metrics(ref, act),
        'windows': windows,
        'spectrum_bands': bands,
        'method': 'One fixed stereo alignment, original sample rate, no time stretching or gain adjustment. All errors use that alignment. Local lag searches are reported only to diagnose drift. Mean-removed errors subtract each compared window’s channel means; these are diagnostic, not independently measured silence offsets.',
        'audio_output': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('actual', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--reference-start', type=float, required=True, help='seconds')
    parser.add_argument('--actual-search-start', type=float, default=0)
    parser.add_argument('--actual-search-end', type=float, required=True, help='seconds')
    parser.add_argument('--seconds', type=float, required=True)
    parser.add_argument('--registration-seconds', type=float, default=2)
    parser.add_argument('--window-seconds', type=float, default=2)
    parser.add_argument('--lag-radius', type=int, default=64)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output already exists; keep previous measurements')
    quantities = [args.reference_start, args.actual_search_start, args.actual_search_end,
                  args.seconds, args.registration_seconds, args.window_seconds]
    if (not all(math.isfinite(x) and x >= 0 for x in quantities)
            or min(args.seconds, args.registration_seconds, args.window_seconds) <= 0
            or args.actual_search_end <= args.actual_search_start or not 0 <= args.lag_radius <= 4096):
        parser.error('invalid interval or lag radius')
    rate, ref = read_pcm(args.reference)
    actual_rate, act = read_pcm(args.actual)
    if rate != actual_rate:
        parser.error('sample rates disagree; comparison never resamples')
    report = compare(ref, act, rate, round(args.reference_start * rate),
                     round(args.actual_search_start * rate), round(args.actual_search_end * rate),
                     round(args.registration_seconds * rate), round(args.seconds * rate),
                     round(args.window_seconds * rate), args.lag_radius)
    for key in ['reference', 'actual']:
        path = getattr(args, key)
        report[key] = {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')
    print(json.dumps({'registration': report['registration'], 'summary': report['summary']}, indent=2))


if __name__ == '__main__':
    main()
