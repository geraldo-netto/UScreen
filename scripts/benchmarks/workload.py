"""Deterministic T382 Tk text/grid workload on one existing X11 display."""
import math
import time
from visibility import VisibilityMonitor


def phases(seconds, warmup):
    result = []
    for trial, pair in enumerate([('static', 'motion'), ('motion', 'static'), ('static', 'motion')], 1):
        for kind in pair:
            result.extend([dict(trial=trial, kind=kind, measured=False, seconds=warmup),
                           dict(trial=trial, kind=kind, measured=True, seconds=seconds)])
    return result


class Workload:
    def __init__(self, geometry, plan, state, event):
        import tkinter as tk

        self.root = tk.Tk()
        self.root.title('Blent T382 reproducible baseline')
        self.root.overrideredirect(True)
        self.root.geometry(geometry)
        self.root.attributes('-topmost', True)
        self.geometry = geometry
        self.canvas = tk.Canvas(self.root, width=1280, height=800, bg='#18212e', highlightthickness=0)
        self.canvas.pack(fill='both', expand=True)
        self.plan, self.state, self.event = plan, state, event
        self.index, self.ticks = -1, 0
        self.lines, self.boxes = [], []
        self.observation_problem = lambda: None
        self.paint()

    def invalidate(self, reason):
        self.state['invalid_reason'] = reason
        self.event({'event': 'invalid', 'reason': reason, 'phase': self.index})
        self.root.destroy()

    def check_visibility(self):
        problem = self.guard.problem() or self.observation_problem()
        if problem:
            self.invalidate(problem)
            return False
        return True

    def watch_visibility(self):
        if self.check_visibility():
            self.root.after(100, self.watch_visibility)

    def paint(self):
        for x in range(0, 1280, 32):
            self.canvas.create_line(x, 0, x, 800, fill='#34455c')
        for y in range(0, 800, 32):
            self.canvas.create_line(0, y, 1280, y, fill='#34455c')
        for row in range(28):
            label = f'{row:02d}  Blent baseline | abcdef 0123456789 | The quick brown fox | 1px lines'
            self.lines.append(self.canvas.create_text(24, row * 32, text=label, anchor='nw',
                                                     font=('DejaVu Sans Mono', 16), fill='#e8edf4'))
        for index, color in enumerate(['#e65b65', '#64d3a2', '#e8c367', '#769df4']):
            self.boxes.append(self.canvas.create_rectangle(1050, index * 180 + 20, 1220,
                                                          index * 180 + 130, fill=color))

    def next_phase(self):
        if not self.check_visibility():
            return
        self.index += 1
        if self.index == len(self.plan):
            self.event({'event': 'complete', 'ticks': self.ticks, 'visibility_verified': True})
            self.root.destroy()
            return
        phase = self.plan[self.index]
        self.started = time.monotonic()
        self.state.update(phase=self.index, **phase, phase_started=self.started, ticks=self.ticks)
        self.event({'event': 'phase', **self.state, 'visibility_verified': True})
        self.draw(0)
        self.root.after(max(1, int(phase['seconds'] * 1000)), self.next_phase)
        self.root.after(1, lambda: self.tick(self.index))

    def draw(self, seconds):
        offset = (seconds * 96) % 896
        for row, item in enumerate(self.lines):
            self.canvas.coords(item, 24, (row * 32 - offset) % 896 - 48)
        for index, item in enumerate(self.boxes):
            x = 1010 + 60 * math.sin(seconds * 2 + index)
            y = index * 180 + 20
            self.canvas.coords(item, x, y, x + 170, y + 110)

    def tick(self, phase_index):
        if phase_index != self.index:
            return
        elapsed = time.monotonic() - self.started
        motion = self.plan[self.index]['kind'] == 'motion'
        if motion:
            self.draw(elapsed)
            self.ticks += 1
            self.state['ticks'] = self.ticks
        wait = max(1, math.ceil((1 / 60 - elapsed % (1 / 60)) * 1000)) if motion else 1000
        self.root.after(wait, lambda: self.tick(phase_index))

    def run(self):
        self.root.update()
        self.guard = VisibilityMonitor(self.root.winfo_id(), self.geometry, take_focus=True)
        try:
            self.guard.start()
            self.root.after(100, self.watch_visibility)
            self.next_phase()
            self.root.mainloop()
        finally:
            self.guard.close()
