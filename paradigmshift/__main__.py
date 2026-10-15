"""Start the editor:  python -m paradigmshift [file.paradigm.json]"""
import sys

from PySide6.QtWidgets import QApplication

from . import theme
from .mainwindow import MainWindow


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("ParadigmShift")
    app.setStyle("Fusion")
    app.setStyleSheet(theme.STYLESHEET)
    app.setWindowIcon(theme.app_icon())
    win = MainWindow()
    win.show()
    if len(sys.argv) > 1:
        win.open_file(sys.argv[1])
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
