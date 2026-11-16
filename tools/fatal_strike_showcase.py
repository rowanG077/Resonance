#!/usr/bin/env python3
"""Validate and assemble native Fatal Strike captures, including partial reviews.

python3 tools/fatal_strike_showcase.py local/review --characters 3,4
Requires FFmpeg with libx264/drawtext and a TrueType font; all game audio comes
from the synchronized production capture, never a replacement soundtrack.
"""
import argparse
import array
import json
import math
import subprocess
import time
import wave
import shutil
import os
import hashlib
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('capture', type=Path)
parser.add_argument('--characters', default='1,2,3,4,5,6,7,8,9')
parser.add_argument('--ffmpeg', default=os.environ.get('RESONANCE_FATAL_FFMPEG') or shutil.which('ffmpeg'))
parser.add_argument('--font', type=Path)
parser.add_argument('--output', type=Path)
args = parser.parse_args()
if not args.ffmpeg:
    parser.error('FFmpeg is required; use --ffmpeg or RESONANCE_FATAL_FFMPEG')
ROOT = args.capture.resolve()
FFMPEG = str(Path(shutil.which(args.ffmpeg) or args.ffmpeg).resolve())
FFPROBE = str(Path(FFMPEG).with_name('ffprobe'))
FONT = args.font or Path(subprocess.check_output(['fc-match', '-f', '%{file}', 'DejaVu Sans'], text=True))
if not FONT.is_file():
    parser.error('A TrueType font is required; use --font')
FONT = FONT.resolve()
NAMES = ['Lloyd', 'Colette', 'Genis', 'Raine', 'Sheena', 'Zelos', 'Presea', 'Regal', 'Kratos']
COLORS = {'Blue': '4aa6ff', 'Red': 'ff5064', 'Green': '4df0a3'}
CHARACTERS = [int(c) for c in args.characters.split(',')]
if not CHARACTERS or len(set(CHARACTERS)) != len(CHARACTERS) or any(c not in range(1,10) for c in CHARACTERS):
    parser.error('--characters must contain unique character IDs from 1 to 9')
TOTAL = len(CHARACTERS)*3
LABELED = ROOT / 'labeled'
LABELED.mkdir(parents=True, exist_ok=True)

def run(args):
    subprocess.run(args, check=True)

def probe(path):
    return json.loads(subprocess.check_output([FFPROBE, '-v', 'error', '-show_streams', '-show_format', '-of', 'json', str(path)]))

clips = []
for character in CHARACTERS:
    name = NAMES[character-1]
    for color in COLORS:
        index = len(clips) + 1
        folder = ROOT / f'{character:02}-{name.lower()}-{color.lower()}'
        deadline = time.monotonic() + 1800
        while True:
            try:
                flow = json.loads((folder / 'flow.json').read_text())
                break
            except (FileNotFoundError, json.JSONDecodeError):
                if time.monotonic() > deadline:
                    raise RuntimeError(f'capture did not complete: {folder.name}')
                time.sleep(3)
        source = folder / 'video-silent.mp4'
        video = next(s for s in probe(source)['streams'] if s['codec_type'] == 'video')
        assert int(video['nb_frames']) == flow['frames'], (folder, video['nb_frames'], flow['frames'])
        assert (video['width'], video['height']) == (1280, 720)
        assert flow['enemy_vanished']
        opening = next(e for e in flow['events'] if e['cue'].startswith('FatalOpening'))
        impact = next(e for e in flow['events'] if e['cue'].startswith('FatalImpact'))
        assert f'color: {color}' in opening['cue'] and 'defeated: true' in impact['cue']
        assert flow['opened_at'] < flow['activated_at'] < flow['impact_at'] < flow['completed_at']
        start = max(0, flow['opened_at'] - 36)
        frames = flow['frames'] - start
        duration = frames / 60
        with wave.open(str(folder / 'audio.wav')) as src:
            rate = src.getframerate()
            assert src.getnchannels() == 2 and rate == 32028
            assert src.getnframes() == flow['frames'] * rate // 60
            src.setpos(start * rate // 60)
            samples = array.array('h', src.readframes(src.getnframes() - src.tell()))
        # Brief fades remove music discontinuities between hard cuts.
        fade = int(rate * 0.025)
        count = len(samples) // 2
        for i in range(fade):
            gain = math.sin(i / fade * math.pi / 2) ** 2
            for channel in range(2):
                samples[2 * i + channel] = round(samples[2 * i + channel] * gain)
                j = 2 * (count - 1 - i) + channel
                samples[j] = round(samples[j] * gain)
        edit = folder / 'audio-edit.wav'
        with wave.open(str(edit), 'wb') as out:
            out.setparams((2, 2, rate, 0, 'NONE', 'not compressed'))
            out.writeframes(samples.tobytes())
        output = LABELED / f'{index:02}-{name.lower()}-{color.lower()}.mp4'
        validated = output.with_suffix('.json')
        signature = hashlib.sha256((folder/'flow.json').read_bytes() + Path(__file__).read_bytes() + str((TOTAL,FONT)).encode()).hexdigest()
        cached = json.loads(validated.read_text()) if validated.exists() else {}
        if cached.get('signature') != signature or not output.exists():
            label = f'{index:02}/{TOTAL}   {name}   {color.upper()}'
            vf = f"drawbox=x=830:y=12:w=436:h=46:color=black@0.85:t=fill,drawbox=x=830:y=12:w=5:h=46:color=0x{COLORS[color]}:t=fill,drawtext=fontfile='{FONT}':text='{label}':fontcolor=white:fontsize=25:x=848:y=24"
            run([FFMPEG, '-hide_banner', '-loglevel', 'error', '-y', '-ss', f'{start/60:.12f}', '-i', str(source), '-i', str(edit), '-vf', vf, '-frames:v', str(frames), '-t', f'{duration:.12f}', '-c:v', 'libx264', '-threads', '2', '-preset', 'veryfast', '-crf', '18', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-ar', '48000', '-b:a', '192k', '-movflags', '+faststart', str(output)])
            check = next(s for s in probe(output)['streams'] if s['codec_type'] == 'video')
            assert int(check['nb_frames']) == frames
        entry = {'signature': signature, 'capture_revision': flow.get('capture_revision'), 'index': index, 'character': name, 'color': color, 'source': folder.name, 'start_frame': start, 'frames': frames, 'duration': duration, 'file': output.name, 'kill_frame': flow['impact_at'] - start}
        validated.write_text(json.dumps(entry, indent=2))
        clips.append(entry)
        (ROOT / 'video-progress.json').write_text(json.dumps(clips, indent=2))
        print(f'SHOWCASE_CLIP_READY {index}/{TOTAL} {name} {color} {frames} frames', flush=True)

offset = 0
chapters = [';FFMETADATA1', f'title=Fatal Strikes — {TOTAL} variations', 'comment=Blue, Red and Green; native stereo effects and voices.']
concat = ['ffconcat version 1.0']
with wave.open(str(ROOT / 'soundtrack.wav'), 'wb') as soundtrack:
    soundtrack.setparams((2, 2, 32028, 0, 'NONE', 'not compressed'))
    for entry in clips:
        frames = entry['frames']
        chapters += ['[CHAPTER]', 'TIMEBASE=1/60', f'START={offset}', f'END={offset+frames}', f"title={entry['index']:02}. {entry['character']} — {entry['color']}"]
        concat += [f"file '{(LABELED / entry['file']).as_posix()}'", f'duration {frames/60:.12f}']
        with wave.open(str(ROOT / entry['source'] / 'audio-edit.wav')) as source:
            pcm = source.readframes(source.getnframes())
        needed = ((offset + frames) * 32028 // 60 - offset * 32028 // 60) * 4
        soundtrack.writeframes((pcm + b'\x00' * max(0, needed - len(pcm)))[:needed])
        entry['chapter_seconds'] = offset / 60
        offset += frames
(ROOT / 'chapters.ffmetadata').write_text('\n'.join(chapters) + '\n')
(ROOT / 'clips.ffconcat').write_text('\n'.join(concat) + '\n')
output = args.output.resolve() if args.output else ROOT / f'fatal-strikes-{TOTAL}-variations.mp4'
output.parent.mkdir(parents=True, exist_ok=True)
run([FFMPEG, '-hide_banner', '-loglevel', 'error', '-y', '-safe', '0', '-f', 'concat', '-i', str(ROOT / 'clips.ffconcat'), '-i', str(ROOT / 'soundtrack.wav'), '-f', 'ffmetadata', '-i', str(ROOT / 'chapters.ffmetadata'), '-map', '0:v:0', '-map', '1:a:0', '-map_metadata', '2', '-map_chapters', '2', '-c:v', 'copy', '-c:a', 'aac', '-ar', '48000', '-b:a', '192k', '-movflags', '+faststart', str(output)])
result = probe(output)
video = next(s for s in result['streams'] if s['codec_type'] == 'video')
assert int(video['nb_frames']) == offset
assert abs(float(video['duration']) - offset / 60) < 0.02
chapter_info = json.loads(subprocess.check_output([FFPROBE, '-v', 'error', '-show_chapters', '-of', 'json', str(output)]))
assert len(chapter_info['chapters']) == TOTAL
(ROOT / 'video-audit.json').write_text(json.dumps({'variations': clips, 'frames': offset, 'duration': offset/60, 'output': str(output), 'streams': result['streams'], 'chapters': chapter_info['chapters']}, indent=2))
(ROOT / 'chapters.txt').write_text('\n'.join(f"{int(e['chapter_seconds'])//60:02}:{int(e['chapter_seconds'])%60:02}  {e['character']} — {e['color']}" for e in clips) + '\n')
print(f'SHOWCASE_COMPLETE {offset} frames {offset/60:.3f} seconds {output}', flush=True)
