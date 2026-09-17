"""T386: benchmark provenance must describe the trials actually requested."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

PATH = Path(__file__).resolve().parents[1] / 'benchmarks/decoder-device.py'
SPEC = importlib.util.spec_from_file_location('decoder_device', PATH)
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class DecoderBenchmarkTests(unittest.TestCase):
    def test_t386_selected_variants_match_provenance_and_trials(self):
        with tempfile.TemporaryDirectory() as directory:
            selected = [('candidate', 'callback-unhinted')]
            args = SimpleNamespace(output=Path(directory) / 'run', serial='test-device',
                                   seconds=30, warmup=5, trials=1, variants=selected)
            with patch.object(BENCH, 'capture', return_value='test fingerprint'), \
                    patch.object(BENCH.subprocess, 'check_output', return_value='test adb'), \
                    patch.object(BENCH, 'trial') as trial:
                BENCH.run(args)
            metadata = json.loads((args.output / 'metadata.json').read_text())
            self.assertEqual(metadata['variants'], [list(item) for item in selected])
            self.assertEqual(trial.call_count, 4)
            self.assertEqual({call.args[1:3] for call in trial.call_args_list}, set(selected))


if __name__ == '__main__':
    unittest.main()
