"""The open paradigm plus undo/redo and selection, shared by all panels."""
from __future__ import annotations

from PySide6.QtCore import QObject, Signal

from .model import Block, Paradigm

UNDO_LIMIT = 200


class Document(QObject):
    changed = Signal()             # anything in the paradigm changed
    selectionChanged = Signal()
    pathChanged = Signal()

    def __init__(self, paradigm: Paradigm | None = None):
        super().__init__()
        self.paradigm = paradigm or Paradigm()
        self.path: str | None = None
        self.dirty = False
        self.selection: list[str] = []
        self._undo: list[dict] = []
        self._redo: list[dict] = []
        self._pending: dict | None = None

    # ---- selection -----------------------------------------------------------
    def select(self, ids: list[str]) -> None:
        ids = [i for i in ids if self.paradigm.block(i)]
        if ids != self.selection:
            self.selection = ids
            self.selectionChanged.emit()

    def selected_blocks(self) -> list[Block]:
        return [b for b in (self.paradigm.block(i) for i in self.selection) if b]

    # ---- undo ----------------------------------------------------------------
    def begin_edit(self) -> None:
        """Remember the state before a (possibly multi-step) interactive edit."""
        if self._pending is None:
            self._pending = self.paradigm.to_dict()

    def commit_edit(self) -> None:
        if self._pending is not None:
            if self._pending != self.paradigm.to_dict():
                self._push(self._pending)
            self._pending = None
        self.changed.emit()

    def cancel_edit(self) -> None:
        if self._pending is not None:
            self._restore(self._pending)
            self._pending = None

    def edit(self, fn) -> None:
        """Run ``fn(paradigm)`` as one undoable step."""
        before = self.paradigm.to_dict()
        fn(self.paradigm)
        if before != self.paradigm.to_dict():
            self._push(before)
        self.changed.emit()

    def _push(self, state: dict) -> None:
        self._undo.append(state)
        del self._undo[:-UNDO_LIMIT]
        self._redo.clear()
        self.dirty = True

    def _restore(self, state: dict) -> None:
        hw = self.paradigm.hardware  # hardware setup is not part of the undo history
        self.paradigm = Paradigm.from_dict(state)
        self.paradigm.hardware = hw
        self.selection = [i for i in self.selection if self.paradigm.block(i)]
        self.changed.emit()
        self.selectionChanged.emit()

    def can_undo(self) -> bool:
        return bool(self._undo)

    def can_redo(self) -> bool:
        return bool(self._redo)

    def undo(self) -> None:
        if self._undo:
            self._redo.append(self.paradigm.to_dict())
            self._restore(self._undo.pop())
            self.dirty = True

    def redo(self) -> None:
        if self._redo:
            self._undo.append(self.paradigm.to_dict())
            self._restore(self._redo.pop())
            self.dirty = True

    # ---- files ---------------------------------------------------------------
    def load(self, path: str) -> None:
        self.paradigm = Paradigm.load(path)
        self.path = path
        self.dirty = False
        self.selection = []
        self._undo.clear()
        self._redo.clear()
        self.changed.emit()
        self.selectionChanged.emit()
        self.pathChanged.emit()

    def save(self, path: str) -> None:
        self.paradigm.save(path)
        self.path = path
        self.dirty = False
        self.pathChanged.emit()

    def reset(self, paradigm: Paradigm) -> None:
        self.paradigm = paradigm
        self.path = None
        self.dirty = False
        self.selection = []
        self._undo.clear()
        self._redo.clear()
        self.changed.emit()
        self.selectionChanged.emit()
        self.pathChanged.emit()
