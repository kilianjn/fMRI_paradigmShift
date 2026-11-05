"""A pseudo-terminal that speaks NordicNeuroLab's SyncBox serial protocol (for tests and screenshots)."""
import os
import threading
import time
import tty


class FakeNNLSyncBox:
    def __init__(self):
        self.master, slave = os.openpty()
        tty.setraw(slave)
        self.device = os.ttyname(slave)
        self._slave = slave
        self.received = []          # commands seen, in order
        self.config = None          # parsed configuration values
        self.running = False
        self._stop = False
        threading.Thread(target=self._serve, daemon=True).start()

    def press(self, key: str):
        os.write(self.master, key.encode())

    def _serve(self):
        buf = b""
        while not self._stop:
            try:
                data = os.read(self.master, 64)
            except OSError:
                return
            buf += data
            while buf:
                if self.config is None and self.received and self.received[-1] == "R":
                    if len(buf) < 48:
                        break
                    payload, buf = buf[:48], buf[48:]
                    self.config = [int(payload[i:i + 4]) for i in range(0, 48, 4)]
                    self.received.append("config")
                    os.write(self.master, payload)
                    continue
                cmd, buf = buf[:1], buf[1:]
                c = cmd.decode("latin-1")
                self.received.append(c)
                os.write(self.master, cmd)                    # every command is echoed
                if c == "R":
                    self.config = None
                elif c == "S":
                    self.running = True
                    if self.config and self.config[11] == 0:  # simulation mode: generate triggers
                        threading.Thread(target=self._simulate, daemon=True).start()
                elif c in ("A", "D"):
                    self.running = False

    def _simulate(self):
        tr = self.config[4] / 1000
        while self.running and not self._stop:
            os.write(self.master, b"s")
            time.sleep(tr)

    def close(self):
        self._stop = True
        self.running = False
        for fd in (self.master, self._slave):
            try:
                os.close(fd)
            except OSError:
                pass
