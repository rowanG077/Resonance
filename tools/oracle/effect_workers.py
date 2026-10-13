"""Resident, silent comparison workers with isolated simulation state per case."""
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import tempfile
import time

from capture import press_key, stop
from state import State, digest
from memory_watch import Watcher
from battle_state import presentation_locations

DOLPHIN_BACKEND = 'Vulkan'
# Xvfb has no DRI3. Render on the GPU while presenting to a headless swapchain.
DOLPHIN_ENV = {'MESA_VK_WSI_DEBUG': 'sw', 'MESA_VK_WSI_HEADLESS_SWAPCHAIN': 'true'}
DOLPHIN_OPTIONS = ['-v', DOLPHIN_BACKEND,
    '-C', 'Dolphin.DSP.Backend=No Audio Output', '-C', 'Dolphin.DSP.Muted=True',
    '-C', 'Dolphin.DSP.DumpAudio=False', '-C', 'Dolphin.Core.EnableCheats=True',
    '-C', 'Dolphin.General.HotkeysRequireFocus=False',
    '-C', 'Graphics.Enhancements.DisableCopyFilter=True',
    '-C', 'Graphics.Hacks.FastTextureSampling=False',
    '-C', 'Graphics.Hacks.EarlyXFBOutput=False',
    '-C', 'Graphics.Hacks.SkipDuplicateXFBs=False',
    '-C', 'Logger.Options.WriteToFile=True', '-C', 'Logger.Options.Verbosity=4',
    '-C', 'Logger.Logs.Video=True', '-C', 'Logger.Logs.Host GPU=True']


def renderer_environment():
    return {key: os.environ[key] for key in ('VK_DRIVER_FILES', 'VK_ICD_FILENAMES',
            'MESA_VK_DEVICE_SELECT', 'DRI_PRIME') if key in os.environ} | DOLPHIN_ENV


def set_movie_backend(path):
    # A DTM's saved video backend overrides Dolphin's command-line selection.
    movie = bytearray(Path(path).read_bytes())
    if len(movie) < 256 or movie[:4] != b'DTM\x1a':
        raise ValueError('invalid Dolphin recording')
    backend_field = slice(81, 97)  # Sixteen-byte videoBackend field in the DTM header.
    movie[backend_field] = DOLPHIN_BACKEND.encode().ljust(16, b'\0')
    Path(path).write_bytes(movie)


def register_boundary(frames, rows, restored_frame=None):
    # A deferred restore presentation has no VI. Accept it only against an
    # exact image collected while the worker was paused without advancing a VI.
    restored_image = (len(frames) == len(rows) + 1 and restored_frame is not None
                      and frames[0].read_bytes() == restored_frame.read_bytes())
    if restored_image:
        frames = frames[1:]
    boundary = len(rows) - len(frames)
    if boundary not in (0, 1):
        raise RuntimeError(f'presentation count differs: {len(rows)} VIs, {len(frames)} images')
    return frames, rows[boundary:], boundary, restored_image


class DolphinFixture:
    def __init__(self, disc, movie, initial_state, output, marker_address, locations, *,
                 disc_sha256, capture_images=True):
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True)
        self.process = self.display = None
        self.watcher = self.alias = None
        self.env = dict(os.environ)
        self.env.update(renderer_environment())
        self.user = self.output/'user'
        self.frames = self.user/'Dump/Frames'
        config = self.user/'Config'
        shutil.copytree(Path(__file__).parent/'config', config)
        shutil.copytree(Path(__file__).parent/'game-settings', self.user/'GameSettings')
        (config/'Hotkeys.ini').write_text(
            '[Hotkeys]\nDevice = XInput2/0/Virtual core pointer\n'
            'General/Toggle Pause = F10\nLoad State/Load State Slot 1 = F1\n'
            'Save State/Save State Slot 2 = F8\n'
            '')
        self.slot = self.user/'StateSaves/GQSEAF.s01'
        self.slot.parent.mkdir()
        self.paused = False
        self.capture_images = capture_images
        self.metadata = {'samples': [], 'complete': False}
        try:
            read_fd, write_fd = os.pipe()
            with (self.output/'display.log').open('w') as log:
                self.display = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0',
                    '800x600x24', '-nolisten', 'tcp'], pass_fds=(write_fd,), stdout=log, stderr=subprocess.STDOUT)
            os.close(write_fd)
            if not select.select([read_fd], [], [], 10)[0]:
                os.close(read_fd)
                raise TimeoutError('Xvfb did not start')
            with os.fdopen(read_fd) as pipe:
                number = pipe.readline().strip()
            if not number.isdecimal():
                raise RuntimeError('Xvfb did not allocate a display')
            self.env.update(DISPLAY=':'+number, QT_QPA_PLATFORM='xcb')
            executable = Path(os.environ.get('RESONANCE_DOLPHIN') or shutil.which('dolphin-emu')).resolve()
            self.alias = tempfile.TemporaryDirectory(prefix='effect-oracle-')
            user_alias = Path(self.alias.name)/'user'
            user_alias.symlink_to(self.user, target_is_directory=True)
            self.marker = marker_address
            self.watcher = Watcher(user_alias, self.output/'memory.jsonl', {
                '80023c10': 'focus_patch_word', '8003efa4': 'secondary_blur_patch_word',
                f'{self.marker:08x}': 'case_marker', **presentation_locations(), **(locations or {})})
            boot_movie = self.output/'input.dtm'
            shutil.copyfile(movie, boot_movie)
            shutil.copyfile(initial_state, str(boot_movie)+'.sav')
            shutil.copyfile(str(initial_state)+'.dtm', str(boot_movie)+'.sav.dtm')
            set_movie_backend(str(boot_movie)+'.sav.dtm')
            command = [str(executable), '-b', '-u', str(user_alias), '-e', str(Path(disc).resolve()),
                       '-m', str(boot_movie), '-s', str(boot_movie)+'.sav'] + DOLPHIN_OPTIONS
            if not capture_images:
                command += ['-C', 'Dolphin.Movie.DumpFrames=False']
            self.metadata.update(command=command, binary_sha256=digest(executable),
                                 observer_address_space='mem1-v2',
                                 disc_sha256=disc_sha256, movie_sha256=digest(movie),
                                 configs={p.name: digest(p) for p in config.iterdir()},
                                 game_settings={p.name: digest(p) for p in (self.user/'GameSettings').iterdir()},
                                 dolphin_version=subprocess.check_output([str(executable), '--version'],
                                     env=dict(self.env, QT_QPA_PLATFORM='offscreen'), text=True,
                                     stderr=subprocess.DEVNULL).strip())
            wrapped = executable.with_name('.'+executable.name+'-wrapped')
            if wrapped.is_file():
                self.metadata['wrapped_binary_sha256'] = digest(wrapped)
            start = time.monotonic()
            with (self.output/'dolphin.log').open('w') as log:
                self.process = subprocess.Popen(command, env=self.env, stdout=log, stderr=subprocess.STDOUT)
            if capture_images:
                self.wait_frames(set(), 12)
            else:
                self.wait_observations(self.watcher.rows, 12)
            self.pause()
            video_log = (self.user/'Logs/dolphin.log').read_text()
            if 'Using Vulkan' not in video_log or 'Video Info:' in video_log:
                raise RuntimeError('Dolphin did not use the requested Vulkan renderer')
            devices = re.findall(r'Using "([^"]+)" with driver: "([^"]+)"', video_log)
            self.metadata['renderer'] = {'backend': DOLPHIN_BACKEND, 'devices': devices,
                                         'environment': renderer_environment()}
            self.metadata['startup_seconds'] = time.monotonic()-start
        except BaseException:
            self.close()
            raise

    def pause(self):
        press_key(self.env, 'F10')
        self.paused = not self.paused
        if self.paused:
            self.drain()

    def drain(self):
        # Settle asynchronous frame dumps and memory observations before counting.
        previous, stable = None, time.monotonic()
        deadline = stable + 10
        while time.monotonic() < deadline:
            current = (self.watcher.rows, len(list(self.frames.glob('framedump_*.png'))))
            if current != previous:
                previous, stable = current, time.monotonic()
            elif time.monotonic() - stable >= 1.:
                return
            time.sleep(.02)
        raise TimeoutError('Dolphin did not settle after pausing')

    def snapshot(self, target):
        """Save a paused live checkpoint without overwriting the input fixture."""
        if not self.paused:
            raise RuntimeError('pause the fixture before saving')
        saved = self.slot.with_suffix('.s02')
        saved.unlink(missing_ok=True)
        press_key(self.env, 'F8')
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError('Dolphin exited before saving the checkpoint')
            try:
                State(saved)
                shutil.copyfile(saved, target)
                shutil.copyfile(str(saved)+'.dtm', str(target)+'.dtm')
                return
            except (OSError, ValueError):
                time.sleep(.05)
        raise TimeoutError('Dolphin did not save a complete checkpoint')

    def wait_frames(self, before, count):
        # Timeout only stalled rendering; parallel captures may run below real time.
        deadline = time.monotonic()+30
        received = 0
        while True:
            if self.process.poll() is not None:
                raise RuntimeError('Dolphin exited; see '+str(self.output/'dolphin.log'))
            current = len(set(self.frames.glob('framedump_*.png'))-before)
            if current >= count+1:
                return
            if current > received:
                received = current
                deadline = time.monotonic()+30
            elif time.monotonic() >= deadline:
                raise TimeoutError(f'Dolphin stalled after {received}/{count+1} fixture frames')
            time.sleep(0.01)

    def wait_observations(self, before, count):
        deadline = time.monotonic() + 30
        received = 0
        while self.watcher.rows - before < count:
            if self.process.poll() is not None:
                raise RuntimeError('Dolphin exited; see ' + str(self.output/'dolphin.log'))
            if self.watcher.error is not None:
                raise RuntimeError(f'MemoryWatcher failed: {self.watcher.error}')
            current = self.watcher.rows - before
            if current > received:
                received = current
                deadline = time.monotonic() + 30
            elif time.monotonic() >= deadline:
                raise TimeoutError(f'Dolphin stalled after {received}/{count} observations')
            time.sleep(0.01)

    def load(self, state):
        if not self.paused:
            raise RuntimeError('fixture must be paused before loading')
        shutil.copyfile(state, self.slot)
        shutil.copyfile(str(state)+'.dtm', str(self.slot)+'.dtm')
        set_movie_backend(str(self.slot)+'.dtm')
        press_key(self.env, 'F1')
        self.drain()

    @staticmethod
    def validate_rows(rows, expected):
        for index, row in enumerate(rows):
            if row['case_marker'] != expected:
                raise RuntimeError('Dolphin did not load the requested effect fixture')
            if any(row[k] != 0x4e800020 for k in ['focus_patch_word', 'secondary_blur_patch_word']):
                raise RuntimeError('the oracle presentation profile was not applied')
            if index and row['presentation_counter'] != rows[index-1]['presentation_counter']+1:
                raise RuntimeError('nonconsecutive field presentation')

    def observe(self, state, target, expected, count):
        """Record named data without image dumping, reusing this paused worker."""
        if self.capture_images or count <= 0:
            raise ValueError('observe requires capture_images=False and a positive count')
        target = Path(target)
        target.mkdir()
        self.load(state)
        first_row = self.watcher.rows
        offset = (self.output/'memory.jsonl').stat().st_size
        start = time.monotonic()
        self.pause()
        self.wait_observations(first_row, count + 4)
        self.pause()
        with (self.output/'memory.jsonl').open() as observations:
            observations.seek(offset)
            rows = [json.loads(line) for line in observations][:count]
        if len(rows) != count:
            raise RuntimeError(f'expected {count} observations, received {len(rows)}')
        self.validate_rows(rows, expected)
        (target/'observations.json').write_text(json.dumps(rows) + '\n')
        sample = {'state_sha256': digest(state), 'observations': count,
                  'seconds': time.monotonic()-start, 'first_vi_sample': first_row,
                  'observations_sha256': digest(target/'observations.json')}
        self.metadata['samples'].append(sample)
        (target/'capture.json').write_text(json.dumps({**self.metadata, 'samples': [sample],
            'complete': True, 'presentation': {'gecko_verified': True, 'images': False},
            'audio': {'backend': 'No Audio Output'}}, indent=2) + '\n')
        return rows

    def warmup(self, state, target, expected, count):
        """Advance setup VIs without claiming an image-to-observation mapping."""
        if count <= 0:
            raise ValueError('warmup requires a positive observation count')
        target = Path(target)
        target.mkdir()
        self.load(state)
        first_row = self.watcher.rows
        start = time.monotonic()
        self.pause()
        self.wait_observations(first_row, count)
        self.pause()
        final_row = dict(self.watcher.latest)
        self.validate_rows([final_row], expected)
        receipt = {'state_sha256': digest(state), 'requested_vis': count,
                   'observed_vis': self.watcher.rows - first_row,
                   'first_vi_sample': first_row, 'last_vi_sample': self.watcher.rows - 1,
                   'seconds': time.monotonic() - start, 'registered': False,
                   'images': False, 'final_observation': final_row}
        self.metadata.setdefault('warmups', []).append(receipt)
        (target/'warmup.json').write_text(json.dumps(receipt, indent=2) + '\n')
        # An image-enabled worker still dumps during setup; none is evidence.
        for frame in self.frames.glob('framedump_*.png'):
            frame.unlink()

    def restore_reference(self, state, target):
        """Flush restored XFBs while paused; never infer a frame from a count."""
        if not self.paused:
            raise RuntimeError('restore reference requires a paused fixture')
        target.mkdir()
        first_row = self.watcher.rows
        reference = None
        receipt = {'state_sha256': digest(state), 'vi_sample': first_row, 'reloads': []}
        # Dolphin 2606 queues an XFB presentation on load and flushes the
        # preceding readback first. Two paused reloads expose a stable reference.
        for index in range(2):
            before = set(self.frames.glob('framedump_*.png'))
            self.load(state)
            if self.watcher.rows != first_row:
                raise RuntimeError('a VI advanced during the paused restore handshake')
            frames = sorted(set(self.frames.glob('framedump_*.png')) - before,
                            key=lambda p: int(p.stem.split('_')[1]))
            if len(frames) > 1:
                raise RuntimeError('multiple images appeared during a paused reload')
            reference = None
            if frames:
                reference = target/f'reload-{index + 1}.png'
                shutil.copyfile(frames[0], reference)
            receipt['reloads'].append({'vi_sample': self.watcher.rows,
                'image': reference.name if reference else None,
                'sha256': digest(reference) if reference else None})
        (target/'restore.json').write_text(json.dumps(receipt, indent=2) + '\n')
        return reference, receipt

    def sequence(self, state, target, expected, count, *, warmup_frames=0):
        """Record one image per VI after reload; reject missing or extra images."""
        if not self.capture_images:
            raise ValueError('sequence requires capture_images=True')
        target = Path(target)
        target.mkdir()
        self.load(state)
        restored_frame, restore_handshake = None, None
        if not warmup_frames:
            restored_frame, restore_handshake = self.restore_reference(state, target/'boundary')
        if warmup_frames:
            # Scene changes may produce VIs without a new image while loading.
            # Start the consecutive-frame check after the requested warmup.
            before = set(self.frames.glob('framedump_*.png'))
            self.pause()
            self.wait_frames(before, warmup_frames)
            self.pause()
        first_row = self.watcher.rows
        observation_offset = (self.output/'memory.jsonl').stat().st_size
        start = time.monotonic()
        before = set(self.frames.glob('framedump_*.png'))
        self.pause()
        self.wait_frames(before, count + 4)
        self.pause()
        frames = sorted(set(self.frames.glob('framedump_*.png')) - before,
                        key=lambda p: int(p.stem.split('_')[1]))
        with (self.output/'memory.jsonl').open() as observations:
            observations.seek(observation_offset)
            rows = [json.loads(line) for line in observations]
        paused_frame = max(before, key=lambda p: int(p.stem.split('_')[1]), default=None)
        reference = restored_frame if restore_handshake is not None else paused_frame
        try:
            frames, rows, boundary, restored_image = register_boundary(frames, rows, reference)
        except RuntimeError:
            (target/'registration-failure.json').write_text(json.dumps({
                'first_vi_sample': first_row, 'observations': rows,
                'frames': [str(frame) for frame in frames],
                'paused_frame': str(paused_frame) if paused_frame else None,
                'restore_handshake': restore_handshake,
            }, indent=2)+'\n')
            raise
        self.validate_rows(rows[:count], expected)
        for index, frame in enumerate(frames[:count]):
            shutil.copyfile(frame, target/f'frame-{index:04}.png')
        (target/'observations.json').write_text(json.dumps(rows[:count])+'\n')
        sample = {'state_sha256': digest(state), 'frames': count,
                  'warmup_frames': warmup_frames,
                  'seconds': time.monotonic()-start, 'first_vi_sample': first_row + boundary,
                  'restored_boundary_without_image': boundary, 'restored_image_removed': restored_image,
                  'restore_handshake': restore_handshake,
                  'observations_sha256': digest(target/'observations.json'),
                  'images': {p.name: digest(p) for p in sorted(target.glob('*.png'))}}
        self.metadata['samples'].append(sample)
        (target/'capture.json').write_text(json.dumps({**self.metadata, 'samples': [sample],
            'complete': True, 'presentation': {'gecko_verified': True, 'texture_sampling': 'precise',
            'one_image_per_vi': True}, 'audio': {'backend': 'No Audio Output'}}, indent=2)+'\n')
        for frame in self.frames.glob('framedump_*.png'):
            frame.unlink()

    def close(self, *, complete=False):
        self.metadata['complete'] = complete
        stop(self.process)
        stop(self.display)
        if self.watcher:
            self.metadata['memory_watch'] = self.watcher.finish()
            self.metadata['complete'] &= self.metadata['memory_watch']['complete']
        if self.alias:
            self.alias.cleanup()
        (self.output/'session.json').write_text(json.dumps(self.metadata, indent=2)+'\n')
        # Failed registration may leave the only image evidence in the raw dump.
        directories = [self.user/'StateSaves']
        if self.metadata['complete']:
            directories.append(self.user/'Dump')
        for directory in directories:
            if directory.exists():
                shutil.rmtree(directory)
        for suffix in ('input.dtm.sav', 'input.dtm.sav.dtm'):
            (self.output/suffix).unlink(missing_ok=True)

        if complete and not self.metadata['complete']:
            raise RuntimeError('Dolphin memory observations were incomplete')


class NativeFixture:
    def __init__(self, executable, cooked, log):
        self.log = Path(log).open('w')
        self.process = subprocess.Popen([str(executable), '--worker', str(cooked)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, bufsize=0)
        self.pending = b''

    def capture(self, sequence, output):
        spec = json.loads(Path(sequence).read_text())
        deadline = time.monotonic() + 120 + spec['updates'] * spec['renders_per_update'] / 20
        self.process.stdin.write((json.dumps({'sequence': str(sequence), 'output': str(output)})+'\n').encode())
        while True:
            while b'\n' in self.pending:
                line, self.pending = self.pending.split(b'\n', 1)
                if line.startswith(b'ORACLE '):
                    if error := json.loads(line[7:])['error']:
                        raise RuntimeError(error)
                    return
                self.log.write(line.decode(errors='replace')+'\n')
            if not select.select([self.process.stdout], [], [], max(0, deadline-time.monotonic()))[0]:
                raise TimeoutError('native capture worker did not finish')
            data = os.read(self.process.stdout.fileno(), 65536)
            if not data:
                raise RuntimeError(f'native capture worker exited ({self.process.poll()})')
            self.pending += data

    def close(self):
        stop(self.process)
        self.log.close()
