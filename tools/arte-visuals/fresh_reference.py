#!/usr/bin/env python3
"""Save a new GQSEAF classroom checkpoint from ordinary boot inputs, silently.

No old state, retail-file edit, or reconstructed ISO is needed. Dolphin mounts
the extracted sys/main.dol and adjacent files/ as a virtual disc. This bounded
bootstrap is separate from arte capture and never retries itself.
"""
import argparse
import json
import os
from pathlib import Path
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import time

ORACLE = Path(__file__).resolve().parents[1] / 'oracle'
sys.path.insert(0, str(ORACLE))
from capture import press_key, stop
from memory_watch import Watcher
from state import State, digest, inspect


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--disc', type=Path, required=True, help='Unmodified extracted disc1/sys/main.dol')
    parser.add_argument('--dolphin', type=Path, required=True, help='Patched Dolphin 2606 executable')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    disc, executable, output = args.disc.resolve(), args.dolphin.resolve(), args.output.resolve()
    if disc.name != 'main.dol' or disc.parent.name != 'sys' or disc.with_name('boot.bin').read_bytes()[:6] != b'GQSEAF':
        raise ValueError('expected an extracted US GameCube sys/main.dol')
    if not output.is_relative_to(Path('local/battle-capture-work').resolve()):
        raise ValueError('fresh reference belongs under local/battle-capture-work')
    output.mkdir(parents=True, exist_ok=False)
    user = output / 'user'
    shutil.copytree(ORACLE / 'config', user / 'Config')
    shutil.copytree(ORACLE / 'game-settings', user / 'GameSettings')
    (user / 'Config/Hotkeys.ini').write_text(
        '[Hotkeys]\nDevice = XInput2/0/Virtual core pointer\nGeneral/Toggle Pause = F10\n'
        'General/Take Screenshot = F9\nSave State/Save State Slot 1 = F8\n')
    case = json.loads((ORACLE / 'cases/new-game-entry.json').read_text())
    case['inputs'] += [dict(poll=p, duration=4, buttons=['a']) for p in range(10000, 29000, 100)]
    write(output / 'boot-input.json', case)
    records = [0x4000] * case['polls']
    for event in case['inputs']:
        bits = sum({'start': 1, 'a': 2, 'down': 128}[button] for button in event['buttons'])
        for poll in range(event['poll'], event['poll'] + event['duration']):
            records[poll] |= bits
    header = bytearray(256)
    header[:10] = b'DTM\x1aGQSEAF'
    header[11], header[151], header[152] = 1, 1, 1
    header[49:58] = b'Resonance'
    for offset in (13, 21):
        struct.pack_into('<Q', header, offset, case['polls'])
    struct.pack_into('<Q', header, 129, case['rtc'])
    struct.pack_into('<Q', header, 237, 2**64 - 1)
    movie = output / 'boot.dtm'
    movie.write_bytes(header + b''.join(struct.pack('<H6B', value, 0, 0, 128, 128, 128, 128) for value in records))
    env = dict(os.environ)
    env.pop('LD_LIBRARY_PATH', None)
    env.update(QT_QPA_PLATFORM='xcb', MESA_VK_WSI_DEBUG='sw', MESA_VK_WSI_HEADLESS_SWAPCHAIN='true')
    version = subprocess.check_output([str(executable), '--version'], env={**env, 'QT_QPA_PLATFORM': 'offscreen'}, text=True).strip()
    if not version.endswith('2606'):
        raise ValueError('the current checkpoint reader requires Dolphin2606')
    process = display = watcher = alias = None
    try:
        read_fd, write_fd = os.pipe()
        display = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '800x600x24', '-nolisten', 'tcp'],
                                   pass_fds=(write_fd,), stdout=(output / 'display.log').open('w'), stderr=subprocess.STDOUT)
        os.close(write_fd)
        if not select.select([read_fd], [], [], 10)[0]:
            raise RuntimeError('Xvfb did not start')
        with os.fdopen(read_fd) as pipe:
            env['DISPLAY'] = ':' + pipe.readline().strip()
        alias = tempfile.TemporaryDirectory(prefix='gc-ref-')
        short = Path(alias.name) / 'user'
        short.symlink_to(user, target_is_directory=True)
        watcher = Watcher(short, output / 'boot-memory.jsonl', {'8035a768 10d0': 'map_id'}, mem1=True)
        command = [str(executable), '-b', '-u', str(short), '-e', str(disc), '-m', str(movie), '-v', 'Vulkan',
                   '-C', 'Dolphin.DSP.Backend=No Audio Output', '-C', 'Dolphin.DSP.Muted=True',
                   '-C', 'Dolphin.DSP.DumpAudio=False', '-C', 'Dolphin.Movie.DumpFrames=False',
                   '-C', 'Dolphin.Core.EmulationSpeed=0.0', '-C', 'Dolphin.General.HotkeysRequireFocus=False']
        process = subprocess.Popen(command, env=env, stdout=(output / 'dolphin.log').open('w'), stderr=subprocess.STDOUT)
        print(json.dumps({'pid': process.pid, 'display': env['DISPLAY'], 'phase': 'fresh_boot'}), flush=True)
        deadline = time.monotonic() + 300
        while watcher.latest.get('presentation_counter', 0) < 11000 or watcher.latest.get('map_id') != 340:
            if process.poll() is not None or watcher.error:
                raise RuntimeError('Dolphin boot stopped: ' + str(watcher.error))
            if time.monotonic() > deadline:
                raise TimeoutError('fresh boot did not reach the observed classroom checkpoint; no automatic retry')
            time.sleep(.1)
        windows = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--pid', str(process.pid)], env=env, text=True).splitlines()
        subprocess.run(['xdotool', 'windowfocus', '--sync', windows[-1]], env=env, check=True)
        press_key(env, 'F9')
        press_key(env, 'F10')
        press_key(env, 'F8')
        saved = user / 'StateSaves/GQSEAF.s01'
        deadline = time.monotonic() + 15
        while True:
            try:
                observation = inspect(State(saved))
                shutil.copyfile(saved, output / 'boot-field.s01')
                shutil.copyfile(str(saved) + '.dtm', output / 'boot-field.s01.dtm')
                break
            except (OSError, ValueError):
                if time.monotonic() > deadline:
                    raise RuntimeError('fresh checkpoint save failed')
                time.sleep(.1)
        write(output / 'boot-field.json', observation)
    finally:
        stop(process)
        stop(display)
        if watcher:
            watcher.finish()
        if alias:
            alias.cleanup()
    entries = [{'path': str(path.relative_to(disc.parent.parent)), 'bytes': path.stat().st_size, 'sha256': digest(path)}
               for path in sorted(disc.parent.parent.rglob('*')) if path.is_file()]
    tree = output / 'extracted-source-tree.json'
    tree.write_text(json.dumps(entries, sort_keys=True, separators=(',', ':')))
    write(output / 'provenance.json', {
        'provenance': 'Original GameCube executable in Dolphin from an extracted virtual disc; fresh boot',
        'source_executable': str(disc), 'source_dol_sha256': digest(disc), 'source_files_modified': False,
        'dolphin': str(executable), 'dolphin_sha256': digest(executable), 'dolphin_version': version,
        'boot_state_sha256': digest(output / 'boot-field.s01'),
        'boot_state_movie_sha256': digest(output / 'boot-field.s01.dtm'),
        'extracted_tree_manifest': tree.name, 'extracted_tree_sha256': digest(tree),
        'runtime_game_code_patches': ['Remove Blur Gecko profile at80023c10 and8003efa4'],
        'assistance': ['DisableCopyFilter', 'Unlimited emulation speed during bootstrap'],
        'command': command, 'simultaneous_dolphin_limit': 1, 'boot_process_closed': True,
    })
    print(json.dumps({'reference': str(output), 'field': observation.get('field')}), flush=True)


if __name__ == '__main__':
    main()
