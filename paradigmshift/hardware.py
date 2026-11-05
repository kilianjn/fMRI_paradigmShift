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

try:  # installed with the app (requirements / pyproject); guarded so the editor still runs without it
    import serial
    import serial.tools.list_ports
except ImportError:  # pragma: no cover
    serial = None

import sys
from collections import Counter

MISSING_SERIAL = ("The serial-port module (pyserial) is not installed. Install it with:  "
                  "pip install pyserial   – then restart ParadigmShift.")
BAUD_RATES = (57600, 115200, 9600, 19200, 38400)       # tried in this order by the auto-detection
SYNCBOX_HINTS = ("syncbox", "sync box", "nordicneurolab", "nordic neuro", "nnl")


def now() -> float:
    return time.perf_counter()


def serial_available() -> bool:
    return serial is not None


def describe_ports() -> list[dict]:
    """All serial ports with their USB details; likely SyncBoxes first."""
    if serial is None:
        return []
    out = []
    for p in serial.tools.list_ports.comports():
        text = " ".join(str(x) for x in (p.description, p.manufacturer, p.product, p.hwid) if x).lower()
        out.append({"device": p.device, "description": p.description or "", "manufacturer": p.manufacturer or "",
                    "vid": p.vid, "pid": p.pid, "likely": any(h in text for h in SYNCBOX_HINTS)})
    out.sort(key=lambda d: (not d["likely"], d["device"]))
    return out


def list_serial_ports() -> list[tuple[str, str]]:
    return [(d["device"], ("★ " if d["likely"] else "") + f"{d['device']} – {d['description']}")
            for d in describe_ports()]


def friendly_serial_error(err: str, port: str = "") -> str:
    """Turn pyserial's exception text into something an operator can act on."""
    e = err.lower()
    where = f"Port {port}" if port else "The port"
    if "pyserial" in e or "no module named 'serial'" in e:
        return MISSING_SERIAL
    if "permission" in e or "access is denied" in e or "zugriff verweigert" in e:
        if sys.platform.startswith("win"):
            return (f"{where} is in use by another program (e.g. NNL software or a second ParadigmShift window). "
                    "Close it and try again.")
        return (f"No permission to open {port or 'the port'}. Add your user to the 'dialout' group "
                "(sudo usermod -aG dialout $USER), then log out and in again.")
    if "busy" in e or "in use" in e:
        return f"{where} is in use by another program (e.g. NNL software). Close it and try again."
    if "filenotfound" in e or "no such file" in e or "cannot find" in e or "could not open port" in e:
        return (f"{where} was not found. Is the SyncBox connected by USB and switched on? "
                "Click “Refresh ports” or “Auto-detect”.")
    return err


def analyse_stream(events: list[tuple[str, float]], trigger_hint: str = "") -> dict:
    """Make sense of characters received from a port: which one is the scanner trigger, what is the TR?

    The trigger is the character that arrives at a steady rhythm (the TR); characters that come irregularly
    are grip-button presses. Returns ``{"chars", "trigger", "tr", "buttons", "printable"}``.
    """
    raw = [c for c, _ in events]
    ok = [c for c in raw if (" " <= c <= "~") or c in "\r\n"]
    printable = len(ok) / len(raw) if raw else 0.0
    chars = [(c, t) for c, t in events if " " < c <= "~"]
    counts = Counter(c for c, _ in chars)
    regular = {}
    for ch, n in counts.items():
        if n < 3:
            continue
        ts = [t for c, t in chars if c == ch]
        d = [b - a for a, b in zip(ts, ts[1:])]
        mean = sum(d) / len(d)
        sd = (sum((x - mean) ** 2 for x in d) / len(d)) ** .5
        if mean > 0.2 and sd / mean < 0.1:
            regular[ch] = mean
    trigger = None
    if trigger_hint in regular or (trigger_hint and trigger_hint in counts and not regular):
        trigger = trigger_hint
    elif regular:
        trigger = max(regular, key=lambda c: counts[c])
    return {"chars": dict(counts), "trigger": trigger, "tr": regular.get(trigger),
            "buttons": [c for c in counts if c != trigger], "printable": printable}


class PortScanner(QThread):
    """Auto-detect: listen on every serial port at the common baud rates and find the one that talks.

    The SyncBox only sends something when a trigger or a grip press happens, so during the scan the
    scanner (or the SyncBox's simulation mode) should be running, or the operator presses grip buttons.
    """
    progress = Signal(str)
    found = Signal(dict)          # {"port", "baud", "description", **analyse_stream(...)}
    finished_scan = Signal(list, list)   # results, errors

    def __init__(self, ports: list[str] | None = None, listen_s: float = 4.0, trigger_hint: str = "",
                 bauds=BAUD_RATES, parent=None):
        super().__init__(parent)
        self.ports = ports
        self.listen_s = listen_s
        self.trigger_hint = trigger_hint
        self.bauds = bauds
        self._stop = False

    def cancel(self):
        self._stop = True

    def run(self):
        if serial is None:
            self.finished_scan.emit([], [MISSING_SERIAL])
            return
        info = {d["device"]: d for d in describe_ports()}
        ports = self.ports if self.ports is not None else list(info)
        results, errors, dead = [], [], set()
        if not ports:
            self.finished_scan.emit([], ["No serial ports found. Is the SyncBox connected by USB and switched on? "
                                         "If it is set to USB keyboard mode, it does not appear as a serial port."])
            return
        for baud in self.bauds:
            if self._stop:
                break
            opened = {}
            for dev in ports:
                if dev in dead:
                    continue
                try:
                    opened[dev] = serial.Serial(dev, baud, timeout=0)
                    opened[dev].reset_input_buffer()
                except Exception as exc:  # noqa: BLE001
                    dead.add(dev)
                    errors.append(f"{dev}: {friendly_serial_error(str(exc), dev)}")
            if not opened:
                break
            events = {dev: [] for dev in opened}
            t_end = now() + self.listen_s
            while now() < t_end and not self._stop:
                self.progress.emit(f"Listening on {', '.join(opened)} at {baud} baud …  {t_end - now():.0f} s")
                for dev, ser in opened.items():
                    try:
                        data = ser.read(256)
                    except Exception as exc:  # noqa: BLE001
                        errors.append(f"{dev}: {friendly_serial_error(str(exc), dev)}")
                        data = b""
                    t = now()
                    events[dev].extend((ch, t) for ch in data.decode("latin-1"))
                self.msleep(5)
            for ser in opened.values():
                try:
                    ser.close()
                except Exception:  # noqa: BLE001
                    pass
            for dev, ev in events.items():
                if not ev:
                    continue
                a = analyse_stream(ev, self.trigger_hint)
                if a["printable"] >= 0.9:
                    res = {"port": dev, "baud": baud, "description": info.get(dev, {}).get("description", ""),
                           **a}
                    results.append(res)
                    self.found.emit(res)
                else:     # bytes arrived but garbled: wrong baud rate, keep trying the others on this port
                    self.progress.emit(f"{dev}: data at {baud} baud looks garbled – trying another speed")
            if results:
                break
        self.finished_scan.emit(results, errors)


class SerialReader(QThread):
    received = Signal(str, float)
    failed = Signal(str)

    def __init__(self, port: str, baud: int, parent=None):
        super().__init__(parent)
        self.port, self.baud = port, baud
        self._stop = False

    def run(self):
        if serial is None:
            self.failed.emit(MISSING_SERIAL)
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
            self.failed.emit(friendly_serial_error(str(exc), self.port))

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
        if self.hw.sync_mode == "serial" and not self.hw.serial_port:
            self.error.emit("Serial mode is selected but no port is set – open Hardware and click Auto-detect.")
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
