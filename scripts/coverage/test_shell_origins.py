"""T497: extracted shell coverage needs immutable, unambiguous provenance."""
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from inventory import syntax_functions
from model import fingerprint
from shell import origins, validate_origin
from shell_origins import attest, canonical, matches, signature


class OriginTest(unittest.TestCase):
    def fixture(self, root):
        source = root/'a.sh'
        source.write_text('choose() {\n  printf safe\n}\n')
        functions = list(syntax_functions(Path('a.sh'), source.read_text(), 'shell'))
        manifest = dict(sources={'a.sh': fingerprint(source)}, functions=[asdict(f) for f in functions])
        path = root/'manifest.json'
        path.write_text(json.dumps(manifest))
        return source, manifest, path

    def test_t497_origin_capture_rejects_source_races_and_preserves_no_script_text(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, manifest, path = self.fixture(root)
            traces = root/'traces'
            output = traces/'origins'
            output.mkdir(parents=True)
            runtime = root/'runtime.sh'
            runtime.write_text(source.read_text() + 'private_not_executed=secret\n')
            digest = fingerprint(runtime)
            attest(runtime, digest, output, root, path)
            self.assertNotIn('secret', ''.join(p.read_text() for p in output.iterdir()))
            self.assertEqual(origins(root, traces, manifest)[digest][0]['file'], 'a.sh')
            runtime.write_bytes(b'\xff')
            with self.assertRaises(ValueError): attest(runtime, digest, output, root, path)
            attest(runtime, fingerprint(runtime), output, root, path)
            self.assertEqual(origins(root, traces, manifest)[fingerprint(runtime)], [])

    def test_t497_duplicate_bodies_and_modified_sources_receive_no_credit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, manifest, _ = self.fixture(root)
            duplicate = root/'b.sh'
            duplicate.write_bytes(source.read_bytes())
            manifest['sources']['b.sh'] = fingerprint(duplicate)
            manifest['functions'].append(dict(manifest['functions'][0], file='b.sh'))
            self.assertEqual(canonical(root, manifest), {})
            self.assertEqual(list(matches(source.read_text(), {})), [])
            duplicate.write_text('changed')
            with self.assertRaises(ValueError): canonical(root, manifest)

    def test_t497_origin_offsets_and_body_signatures_are_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, manifest, _ = self.fixture(root)
            expected = canonical(root, manifest)
            row = next(iter(matches(source.read_text(), expected)))
            for invalid in [None, True, -1, 0, 1 << 31, '1']:
                with self.assertRaises(ValueError): validate_origin(dict(row, at=invalid), expected)
            with self.assertRaises(ValueError): validate_origin(dict(row, body='0'*64), expected)
            for first, last in [(0, 1), (1, 4), (3, 2), (-1, 2)]:
                with self.assertRaises(ValueError): signature(source.read_text(), first, last)

    def test_t497_corrupt_origin_records_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, manifest, _ = self.fixture(root)
            output = root/'origins'
            output.mkdir()
            path = output/('0'*64 + '.json')
            for digest in ['', '../escape', 'z'*64, '1'*64]:
                path.write_text(json.dumps(dict(digest=digest, functions=[])))
                with self.assertRaises(ValueError): origins(root, root, manifest)
            path.unlink()
            (root/'origins.failed').touch()
            with self.assertRaises(ValueError): origins(root, root, manifest)

    def test_t497_seeded_body_mutations_cannot_inherit_original_hits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, manifest, _ = self.fixture(root)
            expected = canonical(root, manifest)
            for index in range(256):
                token = hashlib.sha256(str(index).encode()).hexdigest()
                mutated = source.read_text().replace('safe', token)
                self.assertEqual(list(matches(mutated, expected)), [])
