"""T424 real Tk fixture; isolate Tcl/Xlib lifetime from the test runner."""
import json
import os
from pathlib import Path
import sys
from unittest.mock import patch

from Xlib import X, display

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'benchmarks'))
from visibility import VisibilityMonitor
from workload import Workload


def run(mode):
    events = []
    connection = display.Display()
    cover = connection.screen().root.create_window(101, 101, 50, 50, 0, X.CopyFromParent,
                                                   override_redirect=True)

    def obstruct():
        cover.map()
        cover.configure(stack_mode=X.Above)
        connection.sync()

    factory = lambda xid, geometry, **kwargs: VisibilityMonitor(xid, geometry, lock=lambda: None, **kwargs)
    try:
        with patch('workload.VisibilityMonitor', side_effect=factory):
            duration = 1 if mode == 'occluded' else 0.05
            work = Workload('1280x800+100+100', [dict(seconds=duration, kind='motion', measured=True)], {}, events.append)
            if mode == 'occluded':
                work.root.after(100, obstruct)
            work.run()
        print(json.dumps(events))
    finally:
        connection.close()


if __name__ == '__main__':
    os.environ['DISPLAY'] = sys.argv[1]
    run(sys.argv[2])
