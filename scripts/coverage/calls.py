"""Python invocation evidence: defining a one-line function does not execute it."""
import atexit
import json
import os
from pathlib import Path
import sys
import threading

from copies import Copies


class Calls:
    def __init__(self, root, manifest, destination):
        self.root = root.resolve()
        self.copies = Copies(root, manifest)
        self.destination = destination
        self.known = {str((root / name).resolve()): name for name in manifest if name.endswith('.py')}
        self.code_paths = {}
        self.observed = set()

    def source(self, code):
        if code in self.code_paths:
            return self.code_paths[code]
        path = Path(code.co_filename).resolve()
        original = self.known.get(str(path))
        if original is None:
            tracer = self.copies.file_tracer(str(path))
            if tracer is not None:
                original = self.known.get(tracer.source_filename())
        self.code_paths[code] = original
        return original

    def observe(self, frame, event, argument):
        if event != 'call':
            return
        code = frame.f_code
        source = self.source(code)
        if source is not None:
            self.observed.add((source, code.co_firstlineno, code.co_name))

    def save(self):
        self.destination.mkdir(parents=True, exist_ok=True)
        # PID plus random suffix survives short-lived subprocesses and PID namespaces.
        import tempfile
        with tempfile.NamedTemporaryFile(mode='w', prefix=f'calls-{os.getpid()}-', suffix='.json',
                                   dir=self.destination, delete=False) as stream:
            json.dump(sorted(self.observed), stream)
            stream.write('\n')


def start():
    root = Path(os.environ['BLENT_COVERAGE_ROOT'])
    manifest = json.loads(Path(os.environ['BLENT_COVERAGE_MANIFEST']).read_text())
    calls = Calls(root, manifest['sources'], Path(os.environ['BLENT_PYTHON_CALLS']))
    sys.setprofile(calls.observe)
    threading.setprofile(calls.observe)
    atexit.register(calls.save)
    return calls


def read_calls(directory):
    observed = set()
    from readers import read_bounded
    for path in directory.glob('calls-*.json'):
        observed.update(tuple(row) for row in json.loads(read_bounded(path)))
    return observed


def invoked(function, observed):
    return (function.file, function.entry or function.first, function.name) in observed
