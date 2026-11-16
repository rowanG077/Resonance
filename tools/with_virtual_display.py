"""Run a rendering probe on a private Xvfb display, without using the desktop."""

import argparse
from contextlib import contextmanager
import os
import re
import selectors
import shutil
import signal
import subprocess
import tempfile


def resolution(value):
    match = re.fullmatch(r"([0-9]+)x([0-9]+)", value)
    if match is None or not all(1 <= int(v) <= 8192 for v in match.groups()):
        raise argparse.ArgumentTypeError("expected WIDTHxHEIGHT, dimensions 1..8192")
    return value


@contextmanager
def private_display(size):
    server = shutil.which("Xvfb")
    if server is None:
        raise RuntimeError("Xvfb is unavailable; run this tool inside nix develop")
    read_fd, write_fd = os.pipe()
    with os.fdopen(read_fd, "rb") as ready, tempfile.TemporaryFile() as log:
        try:
            process = subprocess.Popen(
                [server, "-displayfd", str(write_fd), "-screen", "0", f"{size}x24",
                 "-nolisten", "tcp", "-noreset"],
                pass_fds=(write_fd,), stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL, stderr=log, start_new_session=True,
            )
        finally:
            os.close(write_fd)
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(ready, selectors.EVENT_READ)
                if not selector.select(timeout=15):
                    raise RuntimeError("private display did not become ready within 15 seconds")
            number = ready.readline().strip()
            if not number.isdigit() or process.poll() is not None:
                log.seek(0)
                message = log.read().decode(errors="replace")[-2000:]
                raise RuntimeError(f"private display failed to start: {message}")
            env = os.environ.copy()
            env["DISPLAY"] = f":{int(number)}"
            env["WINIT_UNIX_BACKEND"] = "x11"
            env["XDG_SESSION_TYPE"] = "x11"
            for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET"):
                env.pop(name, None)
            # Xvfb selected a free display itself. Never fall back to the user's
            # original DISPLAY, even if the private server subsequently fails.
            yield env
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--size", type=resolution, default="1920x1080")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        parser.error("provide a command after --")

    def interrupted(_signal, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    try:
        with private_display(args.size) as env:
            status = subprocess.run(command, env=env).returncode
            return status if status >= 0 else 128 - status
    except KeyboardInterrupt:
        return 130
    except (OSError, RuntimeError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
