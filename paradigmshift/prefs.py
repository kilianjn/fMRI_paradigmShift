"""User preferences (stored with QSettings, so they survive restarts and are per user)."""
from __future__ import annotations

import os

from PySide6.QtCore import QSettings

DEFAULTS: dict = {
    # general
    "ui_scale": 100,                 # % of the normal text size
    "open_last_file": True,
    "autosave_minutes": 5,           # 0 = off; writes <file>.autosave next to the paradigm
    "show_hints": True,
    "blur_previews": False,          # blur pictures in the editor (thumbnails, picture bin, preview)
    "blur_control_mirror": False,    # blur pictures in the control-room mirror
    # new paradigms / new blocks
    "new_tr": 2.0,
    "new_dummy_scans": 3,
    "new_background": "#000000",
    "new_waiting_screen": "fixation",
    "new_end_text": "Thank you!",
    "rating_mode": "direct",         # how new rating blocks are answered: direct | cursor
    "match_frame": True,             # frame the chosen picture in new matching trials
    # timeline
    "snap": True,
    "snap_px": 10,
    # run & control room
    "data_dir": os.path.join(os.path.expanduser("~"), "ParadigmShift-data"),
    "confirm_abort": True,           # abort needs two clicks / two Esc presses
    "save_paradigm_with_data": True,  # copy of the paradigm next to the logs of every run
    "watchdog_trials": 5,            # warn after this many trials without an answer (0 = off)
    "trigger_warning_trs": 2.5,      # warn when no trigger came for this many TRs
    "control_refresh_ms": 100,
}


class Prefs:
    def __init__(self, settings: QSettings | None = None):
        self.s = settings or QSettings("ParadigmShift", "ParadigmShift")

    def get(self, key: str):
        default = DEFAULTS[key]
        v = self.s.value("prefs/" + key, default)
        try:
            if isinstance(default, bool):
                return v if isinstance(v, bool) else str(v).lower() in ("1", "true", "yes")
            if isinstance(default, int):
                return int(float(v))
            if isinstance(default, float):
                return float(v)
        except (TypeError, ValueError):
            return default
        return v

    def set(self, key: str, value) -> None:
        self.s.setValue("prefs/" + key, value)

    def all(self) -> dict:
        return {k: self.get(k) for k in DEFAULTS}

    def reset(self) -> None:
        for k in DEFAULTS:
            self.s.remove("prefs/" + k)


def apply_block_defaults(p: Prefs) -> None:
    """Make new blocks follow the preferences (e.g. the rating answer mode)."""
    from . import model
    model.PARAM_OVERRIDES.setdefault("rating", {})["mode"] = p.get("rating_mode")
    model.PARAM_OVERRIDES.setdefault("match", {})["show_choice"] = p.get("match_frame")
