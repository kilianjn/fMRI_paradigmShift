"""Input from NordicNeuroLab hardware: SyncBox (scanner trigger) and ResponseGrips.

The NNL SyncBox passes the scanner's TTL pulse – and the ResponseGrip button
presses – on to the stimulus PC either

* as USB keyboard key presses (no driver needed), or
* as characters on a serial/COM port.

Both paths end up in :class:`InputRouter`, which turns raw keys/characters into
``trigger`` and ``button`` signals. A built-in scanner simulator produces
triggers at the TR so paradigms can be tested on any laptop.
"""
from __future__ import annotations

import time

from PySide6.QtCore import QObject, Qt, QThread, QTimer, Signal

from .model import Hardware

try:  # optional dependency – only needed for serial mode
    import serial
    import serial.tools.list_ports
except ImportError:  # pragma: no cover
    serial = None


def now() -> float:
    return time.perf_counter()


def serial_available() -> bool:
    return serial is not None


def list_serial_ports() -> list[tuple[str, str]]:
    if serial is None:
        return []
    return [(p.device, f"{p.device} – {p.description}") for p in serial.tools.list_ports.comports()]


class SerialReader(QThread):
    received = Signal(str, float)
    failed = Signal(str)

    def __init__(self, port: str, baud: int, parent=None):
        super().__init__(parent)
        self.port, self.baud = port, baud
        self._stop = False

    def run(self):
        if serial is None:
            self.failed.emit("pyserial is not installed (pip install pyserial)")
            return
        try:
            with serial.Serial(self.port, self.baud, timeout=0.001) as ser:
                ser.reset_input_buffer()
                while not self._stop:
                    data = ser.read(64)
                    t = now()
                    for ch in data.decode("latin-1"):
                        self.received.emit(ch, t)
        except Exception as exc:  # noqa: BLE001 – surface any port error to the UI
            self.failed.emit(str(exc))

    def stop(self):
        self._stop = True
        self.wait(500)


class InputRouter(QObject):
    trigger = Signal(float)          # timestamp (perf_counter)
    button = Signal(str, float)      # button name, timestamp
    other = Signal(str, float)       # unmapped key
    error = Signal(str)

    def __init__(self, hw: Hardware, simulate_scanner: bool = False, tr: float = 2.0, parent=None):
        super().__init__(parent)
        self.hw = hw
        self._reader: SerialReader | None = None
        self._sim = QTimer(self)
        self._sim.setTimerType(Qt.PreciseTimer)
        self._sim.timeout.connect(lambda: self.trigger.emit(now()))
        self.simulate_scanner = simulate_scanner
        self.tr = tr

    def start(self):
        if self.hw.sync_mode == "serial" and self.hw.serial_port:
            self._reader = SerialReader(self.hw.serial_port, self.hw.baud_rate, self)
            self._reader.received.connect(self.feed)
            self._reader.failed.connect(self.error)
            self._reader.start()

    def start_simulation(self):
        if self.simulate_scanner and self.tr > 0:
            self.trigger.emit(now())
            self._sim.start(int(round(self.tr * 1000)))

    def stop(self):
        self._sim.stop()
        if self._reader:
            self._reader.stop()
            self._reader = None

    def feed(self, text: str, t: float | None = None) -> None:
        """Route a key/character coming from the keyboard or the serial port."""
        if not text:
            return
        t = now() if t is None else t
        if text.lower() == self.hw.trigger_key.lower():
            if not self.simulate_scanner:
                self.trigger.emit(t)
            return
        name = self.hw.button_for_key(text)
        if name:
            self.button.emit(name, t)
        else:
            self.other.emit(text, t)
