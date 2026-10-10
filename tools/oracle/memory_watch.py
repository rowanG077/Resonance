"""Observe game words once per VI through Dolphin's existing MemoryWatcher.

No debugger, CPU pause, memory write, or game patch is involved. The stream
contains raw changed words and a sample for every VI, including unchanged ones.
Only oracle tooling consumes it; the player and importer do not.
"""
import json
import socket
import threading


LOCATIONS = {
    "802c7ea4": "controlled_x_bits",
    "802c7ea8": "controlled_y_bits",
    "802c7eac": "controlled_z_bits",
    "802c7ef8": "controlled_heading_bits",
    "802c8ed8": "camera_x_bits",
    "802c8edc": "camera_y_bits",
    "802c8ee0": "camera_z_bits",
    "802c8ecc": "camera_target_x_bits",
    "802c8ed0": "camera_target_y_bits",
    "802c8ed4": "camera_target_z_bits",
}


class Watcher:
    def __init__(self, user_path, output, extra=None):
        self.locations = dict(LOCATIONS)
        self.aliases = {}
        for address, name in (extra or {}).items():
            address = address.lower()
            if address in self.locations:
                if name != self.locations[address]:
                    self.aliases.setdefault(address, []).append(name)
            else:
                self.locations[address] = name
        directory = user_path / "MemoryWatcher"
        directory.mkdir()
        (directory / "Locations.txt").write_text("\n".join(self.locations) + "\n")
        socket_path = directory / "MemoryWatcher"
        if len(str(socket_path).encode()) >= 104:
            raise ValueError("MemoryWatcher requires a short Unix socket path")
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        self.connection.bind(str(socket_path))
        self.connection.settimeout(0.1)
        self.log = output.open("x")
        self.rows = 0
        self.latest = {}
        self.error = None
        self.stopping = threading.Event()
        self.thread = threading.Thread(target=self.read, name="oracle-memory-watch")
        self.thread.start()

    def read(self):
        values = {name: 0 for name in self.locations.values()}
        values.update({name: 0 for names in self.aliases.values() for name in names})
        try:
            while True:
                try:
                    data, _, flags, _ = self.connection.recvmsg(65536)
                except TimeoutError:
                    if self.stopping.is_set():
                        break
                    continue
                if flags & socket.MSG_TRUNC or not data.endswith(b"\0"):
                    raise ValueError("truncated MemoryWatcher datagram")
                lines = data[:-1].decode("ascii").splitlines()
                if len(lines) % 2:
                    raise ValueError("incomplete MemoryWatcher word pair")
                changes = {}
                for address, value in zip(lines[::2], lines[1::2]):
                    if address not in self.locations or address in changes:
                        raise ValueError("unexpected MemoryWatcher address")
                    changes[address] = int(value, 16)
                    values[self.locations[address]] = changes[address]
                    for name in self.aliases.get(address, []):
                        values[name] = changes[address]
                self.log.write(json.dumps({"vi_sample": self.rows, "changes": changes,
                                           **values}) + "\n")
                self.log.flush()
                self.latest = dict(values)
                self.rows += 1
        except Exception as error:
            self.error = str(error)
        finally:
            self.connection.close()
            self.log.close()

    def finish(self):
        # Call after Dolphin stops; drain all queued observations before exiting.
        self.stopping.set()
        self.thread.join(timeout=2)
        complete = not self.thread.is_alive() and self.error is None and self.rows > 0
        return {"complete": complete, "vi_samples": self.rows,
                "locations": self.locations, "aliases": self.aliases, "game_memory_modified": False,
                "debugger_enabled": False, "error": self.error}
