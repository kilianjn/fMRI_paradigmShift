"""Serial SyncBox support: error messages, stream analysis, auto-detection (with a virtual serial port)."""
import os
import sys
import threading
import time

import pytest

QtWidgets = pytest.importorskip("PySide6.QtWidgets")
pytest.importorskip("serial")
from PySide6.QtCore import QEventLoop, QTimer  # noqa: E402

from paradigmshift import dialogs  # noqa: E402
from paradigmshift.hardware import PortScanner, analyse_stream, friendly_serial_error  # noqa: E402
from paradigmshift.model import Hardware  # noqa: E402

needs_pty = pytest.mark.skipif(not hasattr(os, "openpty") or sys.platform == "darwin",
                               reason="needs a Linux pseudo-terminal")


@pytest.fixture(scope="module")
def app():
    return QtWidgets.QApplication.instance() or QtWidgets.QApplication([])


def fake_syncbox(n=12, period=0.4, buttons=(b"1",)):
    """A pseudo-terminal that behaves like a SyncBox in serial mode."""
    import tty
    master, slave = os.openpty()
    tty.setraw(slave)

    def run():
        for i in range(n):
            try:
                os.write(master, b"s")
                if i % 4 == 2:
                    os.write(master, buttons[i % len(buttons)])
            except OSError:
                return
            time.sleep(period)
    threading.Thread(target=run, daemon=True).start()
    return master, os.ttyname(slave)


def test_analyse_stream_finds_trigger_and_tr():
    ev = [("s", 2.0 * i) for i in range(6)] + [("1", 3.1), ("2", 5.7), ("1", 8.2)]
    a = analyse_stream(sorted(ev, key=lambda e: e[1]))
    assert a["trigger"] == "s" and abs(a["tr"] - 2.0) < 1e-9 and set(a["buttons"]) == {"1", "2"}
    assert a["printable"] == 1.0
    garbled = analyse_stream([(chr(c), i * .1) for i, c in enumerate((0x80, 0xF3, 0x00, 0x9C, 0xFF))])
    assert garbled["printable"] < 0.5
    assert analyse_stream([("5", 0.0)], trigger_hint="5")["trigger"] == "5"     # one pulse + hint


def test_friendly_errors():
    assert "not found" in friendly_serial_error("could not open port 'COM9': FileNotFoundError(2, ...)", "COM9")
    assert "dialout" in friendly_serial_error("[Errno 13] Permission denied: '/dev/ttyUSB0'", "/dev/ttyUSB0") \
        or "in use" in friendly_serial_error("[Errno 13] Permission denied", "COM3")
    assert "pip install pyserial" in friendly_serial_error("No module named 'serial'")


@needs_pty
def test_scanner_finds_fake_syncbox(app):
    master, dev = fake_syncbox()
    sc = PortScanner(ports=[dev], listen_s=2.2, bauds=(57600,))
    out = {}
    loop = QEventLoop()
    sc.finished_scan.connect(lambda r, e: (out.update(r=r, e=e), loop.quit()))
    sc.start()
    QTimer.singleShot(15000, loop.quit)
    loop.exec()
    sc.wait()
    os.close(master)
    (r,) = out["r"]
    assert r["port"] == dev and r["trigger"] == "s" and abs(r["tr"] - 0.4) < 0.05 and "1" in r["buttons"]


@needs_pty
def test_hardware_dialog_autodetect_then_live_triggers(app, monkeypatch):
    master, dev = fake_syncbox(n=40, period=0.3)

    class TestScanner(PortScanner):
        def __init__(self, *a, **kw):
            kw.update(listen_s=2.0)
            super().__init__(*a, **kw)
            self.ports, self.bauds = [dev], (57600,)
    monkeypatch.setattr(dialogs, "PortScanner", TestScanner)
    dlg = dialogs.HardwareDialog(Hardware(), 0.3)
    dlg._auto_detect()
    loop = QEventLoop()
    poll = QTimer()
    poll.timeout.connect(lambda: dlg._scanner is None and loop.quit())
    poll.start(50)
    QTimer.singleShot(15000, loop.quit)
    loop.exec()
    assert dlg.hw.sync_mode == "serial" and dlg.hw.serial_port == dev and dlg.hw.trigger_key == "s"
    assert dlg.rb_ser.isChecked() and "✓" in dlg.detect_info.text()
    before = len(dlg._triggers)                    # router now listens on the detected port
    t_end = time.time() + 1.5
    while time.time() < t_end:
        app.processEvents()
        time.sleep(0.02)
    assert len(dlg._triggers) > before
    dlg.reject()
    os.close(master)


def test_autodetect_recognises_keyboard_mode(app, monkeypatch):
    class NoPorts(PortScanner):
        def __init__(self, *a, **kw):
            kw.update(listen_s=0.3)
            super().__init__(*a, **kw)
            self.ports = []
    monkeypatch.setattr(dialogs, "PortScanner", NoPorts)
    hw = Hardware()
    hw.sync_mode = "serial"
    dlg = dialogs.HardwareDialog(hw, 2.0)
    dlg._auto_detect()
    dlg._scan_keys.extend(["s", "s"])              # keys typed while scanning (SyncBox in keyboard mode)
    loop = QEventLoop()
    poll = QTimer()
    poll.timeout.connect(lambda: dlg._scanner is None and loop.quit())
    poll.start(50)
    QTimer.singleShot(5000, loop.quit)
    loop.exec()
    assert dlg.rb_kb.isChecked() and "keyboard mode" in dlg.detect_info.text()
    dlg.reject()
