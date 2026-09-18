"""T419 SurfaceFlinger presentation timestamps; isolated replay layer only."""
import json
import shlex
import subprocess
import threading
import time


def layer_name(raw, package):
    matches = [line for line in raw.splitlines() if f'SurfaceView[{package}/' in line and '(BLAST)#' in line]
    if len(matches) != 1:
        raise ValueError('replay SurfaceView layer is absent or ambiguous')
    return matches[0].split('{', 1)[-1].split(' parentId', 1)[0].rstrip('}')


def parse(raw):
    lines = raw.splitlines()
    if not lines or not lines[0].isdigit():
        raise ValueError('missing display refresh period')
    rows = [tuple(map(int, line.split())) for line in lines[1:] if line.strip()]
    if any(len(row) != 3 for row in rows):
        raise ValueError('unrecognized SurfaceFlinger frame statistics')
    return dict(period_ns=int(lines[0]), frames=rows)


class PresentationSampler:
    def __init__(self, serial, package, folder):
        self.serial, self.package, self.folder = serial, package, folder
        self.stop = threading.Event()
        self.error = None
        self.thread = threading.Thread(target=self.run, daemon=True)

    def capture(self, command):
        result = subprocess.run(['adb', '-s', self.serial, 'shell', command], capture_output=True,
                                text=True, timeout=5, check=True)
        return result.stdout

    def run(self):
        try:
            name = layer_name(self.capture('dumpsys SurfaceFlinger --list'), self.package)
            with (self.folder / 'surface.jsonl').open('w') as output:
                while not self.stop.is_set():
                    begin = time.monotonic_ns()
                    raw = self.capture('dumpsys SurfaceFlinger --latency ' + shlex.quote(name))
                    row = dict(layer=name, host_ns=begin, collection_ns=time.monotonic_ns() - begin,
                               raw=raw, **parse(raw))
                    output.write(json.dumps(row) + '\n'); output.flush()
                    self.stop.wait(.7)
        except Exception as error:
            self.error = str(error)

    def close(self):
        self.stop.set()
        self.thread.join(timeout=6)
        if self.thread.is_alive():
            self.error = 'presentation collector did not stop'
