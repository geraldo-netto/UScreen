"""Attribute byte-identical isolated Python copies to their maintained source."""
import json
import os
from pathlib import Path

from coverage import CoveragePlugin, FileTracer
from model import fingerprint, verify_sources


class CopyTracer(FileTracer):
    def __init__(self, original):
        self.original = original

    def source_filename(self):
        return self.original


class Copies(CoveragePlugin):
    def __init__(self, root, manifest):
        verify_sources(root, manifest)
        self.root = root.resolve()
        self.sources = {}
        self.owned = {str(self.root / name) for name in manifest if name.endswith('.py')}
        for name, digest in manifest.items():
            if name.endswith('.py'):
                self.sources.setdefault(digest, []).append(str(self.root / name))

    def file_tracer(self, filename):
        path = Path(filename).resolve()
        if path.suffix != '.py' or not path.is_file():
            return None
        # Native and remapped executions must use the same tracer identity;
        # coverage.py rejects mixing its default tracer with a plugin tracer.
        if str(path) in self.owned:
            return CopyTracer(str(path))
        if path.is_relative_to(self.root):
            return None
        matches = self.sources.get(fingerprint(path), [])
        if len(matches) != 1:
            return None
        return CopyTracer(matches[0])

    def file_reporter(self, filename):
        return 'python'


def coverage_init(registry, options):
    manifest = json.loads(Path(os.environ['BLENT_COVERAGE_MANIFEST']).read_text())
    registry.add_file_tracer(Copies(Path(os.environ['BLENT_COVERAGE_ROOT']), manifest['sources']))
