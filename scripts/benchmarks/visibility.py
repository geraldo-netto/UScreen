"""T424: background visibility checks; drawing never waits on logind or X11."""
import os
import subprocess
import threading
import time


def desktop_lock_problem():
    verified = False
    for provider in ('org.cinnamon.ScreenSaver', 'org.gnome.ScreenSaver', 'org.freedesktop.ScreenSaver'):
        try:
            result = subprocess.run(['gdbus', 'call', '--session', '--dest', provider,
                                     '--object-path', '/' + provider.replace('.', '/'),
                                     '--method', provider + '.GetActive'],
                                    capture_output=True, text=True, timeout=1)
        except (OSError, subprocess.TimeoutExpired):
            return 'desktop lock state query failed'
        if result.returncode:
            continue
        if result.stdout.strip() == '(true,)':
            return 'desktop locked'
        if result.stdout.strip() != '(false,)':
            return 'desktop lock state is unknown'
        verified = True
    return None if verified else 'no supported desktop lock provider'


def lock_problem():
    session = os.environ.get('XDG_SESSION_ID')
    if not session:
        return 'session lock state unavailable: XDG_SESSION_ID is unset'
    try:
        result = subprocess.run(['loginctl', 'show-session', session, '-p', 'LockedHint', '--value'],
                                capture_output=True, text=True, timeout=1)
    except (OSError, subprocess.TimeoutExpired):
        return 'session lock state query failed'
    if result.returncode or result.stdout.strip() not in ('yes', 'no'):
        return 'session lock state is unknown'
    return 'desktop locked' if result.stdout.strip() == 'yes' else desktop_lock_problem()


class VisibilityMonitor:
    def __init__(self, window_id, geometry, probe=None, lock=lock_problem, take_focus=False):
        if probe is None:
            from xvisibility import XVisibility
            probe = lambda: XVisibility(window_id, geometry)
        self.probe, self.lock = probe, lock
        self.take_focus = take_focus
        self.stop, self.ready = threading.Event(), threading.Event()
        self.last_checked, self.failure = 0, None
        self.thread = threading.Thread(target=self.run, daemon=True, name='baseline-visibility')

    def start(self):
        self.thread.start()
        self.ready.wait(timeout=3)

    def run(self):
        probe = None
        try:
            probe = self.probe()
            self.failure = self.lock()
            if self.failure:
                self.ready.set()
                return
            if self.take_focus:
                probe.focus()
            while not self.stop.is_set():
                self.failure = self.lock() or probe.problem()
                self.last_checked = time.monotonic()
                self.ready.set()
                if self.failure:
                    return
                self.stop.wait(0.25)
        except Exception as error:
            self.failure = f'visibility query failed: {error}'
            self.ready.set()
        finally:
            if probe is not None:
                probe.close()

    def problem(self):
        if self.failure:
            return self.failure
        if time.monotonic() - self.last_checked > 2:
            return 'visibility check missing or stale'
        return None

    def close(self):
        self.stop.set()
        if self.thread.ident is not None:
            self.thread.join(timeout=2)
