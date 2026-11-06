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


# ------------------------------------------------------------------ NNL protocol (computer mode)
from paradigmshift.hardware import (InputRouter, SerialReader, nnl_payload,  # noqa: E402
                                    nnl_probe)


def nnl_box():
    sys.path.insert(0, os.path.dirname(__file__))
    from fake_syncbox import FakeNNLSyncBox
    return FakeNNLSyncBox()


def wait_until(app, cond, timeout=6.0):
    end = time.time() + timeout
    while time.time() < end and not cond():
        app.processEvents()
        time.sleep(0.02)
    return cond()


def test_nnl_payload_layout():
    p = nnl_payload(volumes=154, tr_ms=2000, simulation=True, slices=36)
    assert len(p) == 48
    vals = [int(p[i:i + 4]) for i in range(0, 48, 4)]
    assert vals == [0, 154, 36, 100, 2000, 1, 1, 0, 0, 0, 0, 0]
    assert int(nnl_payload(volumes=20000, tr_ms=1)[4:8]) == 9999 and nnl_payload(1, 1)[-4:] == b"0001"


@needs_pty
def test_probe_tells_syncbox_from_other_devices(app):
    box = nnl_box()
    assert nnl_probe(box.device) and box.received[:2] == ["C", "D"]     # asked, then returned to manual
    master, other = fake_syncbox(n=3)                                  # a device that only streams
    assert not nnl_probe(other, timeout=0.3)
    box.close()
    os.close(master)


@needs_pty
def test_scanner_finds_nnl_box_by_handshake(app):
    box = nnl_box()
    sc = PortScanner(ports=[box.device], listen_s=1.0, bauds=(57600,))
    out = {}
    sc.finished_scan.connect(lambda r, e: out.update(r=r))
    sc.start()
    assert wait_until(app, lambda: "r" in out, 10)
    sc.wait()
    assert out["r"][0]["protocol"] == "nnl" and out["r"][0]["port"] == box.device
    box.close()


@needs_pty
def test_controlled_session_configures_starts_and_stops(app):
    from paradigmshift.model import Hardware
    box = nnl_box()
    hw = Hardware(sync_mode="serial", serial_port=box.device)
    for i, b in enumerate(hw.buttons):
        b.key = "abcd"[i]
    router = InputRouter(hw, simulate_scanner=True, tr=0.3, volumes=77)
    trig, status, names = [], [], []
    router.button.connect(lambda n, _t: names.append(n))
    router.trigger.connect(trig.append)
    router.status.connect(status.append)
    router.start()
    router.start_simulation()                       # must not add software triggers – the box simulates
    assert wait_until(app, lambda: len(trig) >= 3, 8)
    assert box.config[1] == 77 and box.config[4] == 300 and box.config[11] == 0
    assert box.received[:4] == ["C", "R", "config", "S"] and "simulation" in status[0]
    box.press("b")
    assert wait_until(app, lambda: names == ["Left index"], 3)
    router.stop()
    assert box.received[-2:] == ["A", "D"]          # stopped and back in manual mode
    box.close()


@needs_pty
def test_handshake_failure_is_reported(app):
    from paradigmshift.model import Hardware
    master, dev = fake_syncbox(n=2)                 # streams, but never answers commands
    errs = []
    r = SerialReader(dev, 57600, control=dict(volumes=10, tr_ms=2000))
    r.failed.connect(errs.append)
    r.start()
    assert wait_until(app, lambda: bool(errs), 6)
    r.wait()
    assert "did not confirm" in errs[0]
    os.close(master)


@needs_pty
def test_dialog_detects_nnl_box_and_simulates_with_it(app, monkeypatch):
    box = nnl_box()

    class Only(PortScanner):
        def __init__(self, *a, **kw):
            super().__init__(*a, **kw)
            self.ports, self.bauds = [box.device], (57600,)
    monkeypatch.setattr(dialogs, "PortScanner", Only)
    dlg = dialogs.HardwareDialog(Hardware(), 0.3)
    dlg._auto_detect()
    assert wait_until(app, lambda: dlg._scanner is None, 10)
    assert dlg.hw.syncbox_control and dlg.ctrl.isChecked() and dlg.hw.trigger_key == "s"
    assert [b.key for b in dlg.hw.buttons] == ["a", "b", "c", "d"]
    dlg.sim_btn.setChecked(True)                    # SyncBox's own simulation
    assert wait_until(app, lambda: len(dlg._triggers) >= 3, 8)
    assert box.config[11] == 0
    dlg.reject()
    assert box.received[-1] == "D"
    box.close()


@needs_pty
def test_full_run_with_syncbox_simulation(app, tmp_path):
    import csv

    from paradigmshift.controlroom import open_run
    from paradigmshift.model import Block, Paradigm
    box = nnl_box()
    par = Paradigm(tr=0.25, dummy_scans=1)
    par.hardware.sync_mode, par.hardware.serial_port = "serial", box.device
    for i, b in enumerate(par.hardware.buttons):
        b.key = "abcd"[i]
    par.blocks += [Block.create("fixation", 0, 1.0)]
    c, pw, cr = open_run(par, {"sub": "x", "run": 1, "out_dir": str(tmp_path), "simulate": True,
                               "fullscreen": False, "screen": 0, "save_paradigm_with_data": False})
    assert wait_until(app, lambda: c.phase == "running", 8)
    box.press("b")
    # the backup log is written the moment the press happens – before the run is over
    assert wait_until(app, lambda: "Left index" in open(c.press_path).read(), 3) and c.phase == "running"
    assert wait_until(app, lambda: c.phase == "done", 8)
    rows = list(csv.DictReader(open(c.press_path), delimiter="\t"))
    assert [(r["key"], r["button"], r["block"]) for r in rows] == [("b", "Left index", "Fixation")]
    assert c.press_path in c.files
    assert box.config[1] == par.volumes + 1 + 20 and box.received[-2:] == ["A", "D"]
    assert "computer mode" in c.hw_status
    cr.close()
    box.close()
