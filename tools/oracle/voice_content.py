#!/usr/bin/env python3
"""Read-only voiced-content registration against finalized WAV recordings.

This measures existing PCM; it neither cooks assets nor opens an audio device.
Correlation identifies content, not a bit-exact or perceptual acceptance gate.
"""
import argparse
import hashlib
import json
from pathlib import Path
import wave

import numpy as np
from scipy.signal import correlate


def read_channels(path):
    with wave.open(str(path)) as source:
        if source.getsampwidth() != 2:
            raise ValueError("expected PCM16 recording")
        pcm = np.frombuffer(source.readframes(source.getnframes()), dtype="<i2")
        return pcm.reshape(-1, source.getnchannels()).astype(np.float64), source.getframerate()


def read(path):
    pcm, rate = read_channels(path)
    return pcm.mean(axis=1), rate


def passage_profile(recording, template, rate, match):
    """Inspect the entire spoken line at one alignment, including its end.

    The oracle contains music and effects too: residuals include that background.
    Independent realignment of each window would hide dropouts or timing drift.
    """
    start, gain = match["frame"], match["gain"]
    available = min(len(template), len(recording) - start)
    windows = []
    width = rate // 4
    for at in range(0, available, width):
        expected = template[at:min(at + width, available)]
        observed = recording[start + at:start + at + len(expected)]
        power = float(np.dot(expected, expected))
        if power < 16 ** 2 * len(expected):
            continue
        dot = float(np.dot(expected, observed))
        windows.append({"offset_frames": at, "frames": len(expected),
                        "source_rms_pcm16": float(np.sqrt(power / len(expected))),
                        "correlation": dot / np.sqrt(max(1., power * np.dot(observed, observed))),
                        "gain": dot / power,
                        "residual_rms_pcm16": float(np.sqrt(np.mean((observed - gain * expected) ** 2)))})
    return {"source_frames": len(template), "available_frames": available,
            "fixed_alignment": start, "fixed_prefix_gain": gain, "windows": windows,
            "minimum_correlation": min((w["correlation"] for w in windows), default=None),
            "median_correlation": float(np.median([w["correlation"] for w in windows])) if windows else None}


def locate(recording, template):
    # Search at the original sample rate. Decimating without a low-pass filter
    # can register a different passage when a short prefix contains high tones.
    a, b = recording, template
    if len(a) < len(b):
        return None
    dot = correlate(a, b, mode="valid", method="fft")
    power = np.concatenate(([0.], np.cumsum(a * a)))
    energy = power[len(b):] - power[:-len(b)]
    score = dot / np.sqrt(np.maximum(energy, 1.) * np.dot(b, b))
    peak = int(np.argmax(score))
    observed = recording[peak:peak + len(template)]
    gain = float(np.dot(observed, template) / np.dot(template, template))
    residual = observed - template * gain
    return {"frame": peak, "frames": len(template), "correlation": float(score[peak]),
            "gain": gain, "residual_rms_pcm16": float(np.sqrt(np.mean(residual * residual)))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, default=Path("local/cooked"))
    parser.add_argument("--recording", type=Path, action="append", required=True)
    voices = parser.add_mutually_exclusive_group(required=True)
    voices.add_argument("--voice", type=int, action="append")
    voices.add_argument("--all", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--full-profile", action="store_true",
                        help="Also measure each quarter-second of the full line at a fixed alignment")
    args = parser.parse_args()
    manifest = args.assets / "fields/iselia-classroom-audio.json"
    spec = json.loads(manifest.read_text())
    templates = {}
    for voice in (map(int, spec["voices"]) if args.all else args.voice):
        asset = spec["voices"][str(voice)]
        path = args.assets / asset["path"]
        if hashlib.sha256(path.read_bytes()).hexdigest() != asset["sha256"]:
            raise ValueError("voice asset hash mismatch")
        pcm, rate = read(path)
        # Use a short prefix that remains present even when dialogue is advanced.
        count = min(len(pcm), rate * 3 // 10)
        if not np.any(pcm[:count]):
            raise ValueError("silent voice prefix")
        templates[voice] = (pcm, count, rate, asset["sha256"])
    result = {"method": "PCM16 mono projection; normalized FFT registration at the original sample rate",
              "audio_device": False, "assets_modified": False,
              "manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(), "recordings": []}
    for path in args.recording:
        channels, rate = read_channels(path)
        pcm = channels.mean(axis=1)
        record = {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                  "sample_rate": rate, "frames": len(pcm), "voices": {}}
        for voice, (template, count, expected_rate, digest) in templates.items():
            if rate != expected_rate:
                raise ValueError("sample-rate mismatch; this tool does not resample")
            match = locate(pcm, template[:count])
            record["voices"][str(voice)] = {"source_sha256": digest, "match": match}
            if args.full_profile and match:
                record["voices"][str(voice)]["full_passage"] = passage_profile(pcm, template, rate, match)
                record["voices"][str(voice)]["channels"] = [
                    passage_profile(channel, template, rate, match) for channel in channels.T]
        result["recordings"].append(record)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
