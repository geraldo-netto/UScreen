"""T399: reject misleading replay provenance before any visual workload."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

PATH = Path(__file__).resolve().parents[1] / 'benchmarks/decoder-plan.py'
SPEC = importlib.util.spec_from_file_location('decoder_plan', PATH)
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class DecoderPlanTests(unittest.TestCase):
    def rejected(self, rate, installed):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            plan = root / 'plan.json'
            plan.write_text(json.dumps([dict(scene='motion', fixture='clip.bin', profile='legacy', rate=rate,
                                             trial=0, seconds=20, warmup=5)]))
            provenance = root / 'provenance.json'
            provenance.write_text(json.dumps(dict(package='com.uscreen.decoderbench.candidate', apk_sha256='a' * 64)))
            args = SimpleNamespace(plan=plan, provenance=provenance, output=root / 'output', serial='device')
            responses = ['fingerprint', 'package:/data/app/replay/base.apk\n', installed + '  /data/app/replay/base.apk\n']
            with patch.object(BENCH.DEVICE, 'capture', side_effect=responses), patch.object(BENCH.DEVICE, 'trial') as trial:
                with self.assertRaises(ValueError):
                    BENCH.run(args)
                trial.assert_not_called()

    def test_t399_invalid_burst_or_profile_cannot_mislabel_measurement(self):
        row = dict(scene='motion', fixture='clip.bin', profile='legacy', rate=60, trial=0, seconds=10, warmup=2)
        for update in [dict(burst=1.5), dict(burst=True), dict(profile='typo-profile')]:
            with self.subTest(update=update), self.assertRaises(ValueError):
                BENCH.validate([dict(row, **update)])

    def test_t399_wrong_installed_apk_cannot_claim_provenance(self):
        self.rejected(60, 'b' * 64)

    def test_t399_out_of_range_rate_cannot_be_silently_clamped(self):
        self.rejected(120, 'a' * 64)


if __name__ == '__main__':
    unittest.main()
