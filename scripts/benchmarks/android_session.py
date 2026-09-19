"""Bounded, read-only tablet process/foreground checks for full-device baselines."""
import json
import re
import subprocess
import threading
import time
from observe import process_stat

QUERY = ('p=$(pidof io.github.geraldo_netto.uscreen); test -n "$p" || exit 1; '
         'printf "USCREEN_STAT "; run-as io.github.geraldo_netto.uscreen cat /proc/$p/stat || exit 1; '
         'dumpsys window displays; dumpsys window policy')


def parse_snapshot(output):
    stat = next((line[len('USCREEN_STAT '):] for line in output.splitlines()
                 if line.startswith('USCREEN_STAT ')), None)
    if stat is None:
        raise ValueError('tablet process identity unavailable')
    process = process_stat(stat, 1)
    focus = re.findall(r'mCurrentFocus=Window\{[^\n}]*\s([\w.]+)/[\w.$]+\}', output)
    if focus != ['io.github.geraldo_netto.uscreen']:
        raise ValueError('UScreen is not the verified foreground tablet window')
    if re.search(r'(?:mShowingLockscreen|mKeyguardShowing|isKeyguardShowing)\s*=\s*true', output):
        raise ValueError('tablet keyguard is showing')
    return dict(pid=process['pid'], start_ticks=process['start_ticks'], foreground=True)


def snapshot(serial):
    result = subprocess.run(['adb', '-s', serial, 'shell', QUERY], capture_output=True, text=True, timeout=2)
    if result.returncode:
        raise ValueError(f'tablet session query failed ({result.returncode})')
    return parse_snapshot(result.stdout)


class AndroidSessionMonitor:
    def __init__(self, serial, pid, folder, probe=snapshot):
        self.serial, self.pid, self.folder, self.probe = serial, pid, folder, probe
        self.failure, self.last_checked, self.identity = None, 0, None
        self.stop, self.ready = threading.Event(), threading.Event()
        self.thread = threading.Thread(target=self.run, name='baseline-android-session', daemon=True)

    def start(self):
        self.thread.start()
        self.ready.wait(timeout=3)

    def validate(self, value):
        identity = (value['pid'], value['start_ticks'])
        if value['pid'] != self.pid or (self.identity is not None and identity != self.identity):
            raise ValueError('tablet process changed during measurement')
        if value.get('foreground') is not True:
            raise ValueError('tablet foreground verification failed')
        self.identity = identity

    def run(self):
        try:
            with (self.folder / 'android-session.jsonl').open('w') as output:
                while not self.stop.is_set():
                    value = self.probe(self.serial)
                    self.validate(value)
                    self.last_checked = time.monotonic()
                    output.write(json.dumps(dict(utc=time.time(), monotonic=self.last_checked, **value)) + '\n')
                    output.flush()
                    self.ready.set()
                    self.stop.wait(.5)
        except Exception as error:
            self.failure = f'Android observation failed: {error}'
        finally:
            self.ready.set()

    def problem(self):
        if self.failure:
            return self.failure
        if time.monotonic() - self.last_checked > 4:
            return 'Android session observation missing or stale'
        return None

    def close(self):
        self.stop.set()
        if self.thread.ident is not None:
            self.thread.join(timeout=3)
        if self.thread.is_alive():
            self.failure = 'Android session observer did not stop'
