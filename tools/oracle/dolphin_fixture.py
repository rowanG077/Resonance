"""One silent Dolphin process, reloading copied renderer states between samples."""
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import tempfile
import time

from capture import press_key, stop
from state import digest
from memory_watch import Watcher


class DolphinFixture:
    def __init__(self, disc, movie, initial_state, output, marker_address):
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True)
        self.process = self.display = None
        self.watcher = self.alias = None
        self.env = dict(os.environ)
        self.user = self.output/'user'
        self.frames = self.user/'Dump/Frames'
        config = self.user/'Config'
        shutil.copytree(Path(__file__).parent/'config', config)
        shutil.copytree(Path(__file__).parent/'game-settings', self.user/'GameSettings')
        (config/'Hotkeys.ini').write_text(
            '[Hotkeys]\nDevice = XInput2/0/Virtual core pointer\n'
            'General/Toggle Pause = F10\nLoad State/Load State Slot 1 = F1\n')
        self.slot = self.user/'StateSaves/GQSEAF.s01'
        self.slot.parent.mkdir()
        self.paused = False
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
            executable = Path(shutil.which('dolphin-emu')).resolve()
            self.alias = tempfile.TemporaryDirectory(prefix='effect-oracle-')
            user_alias = Path(self.alias.name)/'user'
            user_alias.symlink_to(self.user, target_is_directory=True)
            self.marker = marker_address
            self.watcher = Watcher(user_alias, self.output/'memory.jsonl', {
                '80023c10': 'focus_patch_word', '8003efa4': 'secondary_blur_patch_word',
                f'{self.marker:08x}': 'case_marker'})
            boot_movie = self.output/'input.dtm'
            shutil.copyfile(movie, boot_movie)
            shutil.copyfile(initial_state, str(boot_movie)+'.sav')
            shutil.copyfile(str(initial_state)+'.dtm', str(boot_movie)+'.sav.dtm')
            command = [str(executable), '-b', '-u', str(user_alias), '-e', str(Path(disc).resolve()),
                       '-m', str(boot_movie), '-s', str(boot_movie)+'.sav', '-v', 'OGL',
                       '-C', 'Dolphin.DSP.Backend=No Audio Output', '-C', 'Dolphin.DSP.Muted=True',
                       '-C', 'Dolphin.DSP.DumpAudio=False', '-C', 'Dolphin.Core.EnableCheats=True',
                       '-C', 'Dolphin.General.HotkeysRequireFocus=False',
                       '-C', 'Graphics.Enhancements.DisableCopyFilter=True',
                       '-C', 'Graphics.Hacks.FastTextureSampling=False']
            self.metadata.update(command=command, binary_sha256=digest(executable),
                                 disc_sha256=digest(disc), movie_sha256=digest(movie),
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
            self.wait_frames(set(), 12)
            self.pause()
            self.metadata['startup_seconds'] = time.monotonic()-start
        except BaseException:
            self.close()
            raise

    def pause(self):
        press_key(self.env, 'F10')
        self.paused = not self.paused

    def wait_frames(self, before, count):
        deadline = time.monotonic()+30
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError('Dolphin exited; see '+str(self.output/'dolphin.log'))
            frames = sorted(set(self.frames.glob('framedump_*.png'))-before,
                            key=lambda p: int(p.stem.split('_')[1]))
            if len(frames) >= count+1:
                return frames[count-1]
            time.sleep(0.01)
        raise TimeoutError('Dolphin did not render the fixture')

    def capture(self, state, target, expected):
        start = time.monotonic()
        if not self.paused:
            raise RuntimeError('fixture must be paused before loading')
        shutil.copyfile(state, self.slot)
        shutil.copyfile(str(state)+'.dtm', str(self.slot)+'.dtm')
        before = set(self.frames.glob('framedump_*.png'))
        press_key(self.env, 'F1')
        self.pause()
        frame = self.wait_frames(before, 12)
        self.pause()
        shutil.copyfile(frame, target)
        last = self.watcher.latest
        if self.watcher.error or any(last.get(k) != 0x4e800020 for k in ['focus_patch_word', 'secondary_blur_patch_word']):
            raise RuntimeError('the oracle presentation profile was not applied')
        if last.get('case_marker') != expected:
            raise RuntimeError('Dolphin did not load the requested effect fixture')
        self.metadata['samples'].append({'state_sha256': digest(state), 'image_sha256': digest(target),
                                        'seconds': time.monotonic()-start})
        metadata = {**self.metadata, 'samples': self.metadata['samples'][-1:], 'complete': True,
                    'requested_frame': 12, 'presentation': {'gecko_verified': True, 'texture_sampling': 'precise'},
                    'audio': {'backend': 'No Audio Output'}, 'session': str(self.output)}
        Path(target).with_name('capture.json').write_text(json.dumps(metadata, indent=2)+'\n')
        for frame in self.frames.glob('framedump_*.png'):
            frame.unlink()

    def close(self):
        stop(self.process)
        stop(self.display)
        if self.watcher:
            self.metadata['memory_watch'] = self.watcher.finish()
            self.metadata['complete'] &= self.metadata['memory_watch']['complete']
        if self.alias:
            self.alias.cleanup()
        (self.output/'session.json').write_text(json.dumps(self.metadata, indent=2)+'\n')

    def __enter__(self):
        return self

    def __exit__(self, kind, value, traceback):
        self.metadata['complete'] = kind is None
        self.close()
        if kind is None and not self.metadata['complete']:
            raise RuntimeError('Dolphin memory observations were incomplete')
