"""T386: benchmark provenance must describe the trials actually requested."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
import xml.etree.ElementTree as ET
from unittest.mock import patch

PATH = Path(__file__).resolve().parents[1] / 'benchmarks/decoder-device.py'
SPEC = importlib.util.spec_from_file_location('decoder_device', PATH)
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class DecoderBenchmarkTests(unittest.TestCase):
    def test_t460_replay_can_remain_visible_over_the_same_keyguard_as_uscreen(self):
        path = PATH.with_name('decoder-project.py')
        spec = importlib.util.spec_from_file_location('decoder_project', path)
        project = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(project)
        android = '{http://schemas.android.com/apk/res/android}'
        production = ET.parse(project.ROOT / 'android/app/src/main/AndroidManifest.xml')
        activity = next(a for a in production.findall('./application/activity')
                        if a.get(android + 'name') == '.MainActivity')
        replay = ET.fromstring(project.MANIFEST).find('./application/activity')
        for attribute in ['showWhenLocked', 'turnScreenOn']:
            self.assertEqual(activity.get(android + attribute), 'true')
            self.assertEqual(replay.get(android + attribute), activity.get(android + attribute),
                             'T460: keyguard must not retire the benchmark Surface at launch')
        self.assertEqual(replay.get(android + 'permission'), 'android.permission.DUMP')

    def test_t458_generated_replay_includes_current_decoder_dependencies(self):
        path = PATH.with_name('decoder-project.py')
        spec = importlib.util.spec_from_file_location('decoder_project', path)
        project = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(project)
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(directory=Path(directory) / 'replay', revision=None,
                                   package='com.uscreen.decoderbench.candidate')
            output = project.prepare(args)
            self.assertEqual((output / 'app/src/main/java/VideoCodec.kt').read_text(),
                             (project.SOURCE / 'VideoCodec.kt').read_text())
            self.assertIn('org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3',
                          (output / 'app/build.gradle.kts').read_text())
            for name in ['MediaProfiles.kt', 'MediaInventory.kt', 'DecoderSelection.kt']:
                self.assertEqual((output / 'app/src/main/java' / name).read_text(),
                                 (project.SOURCE / name).read_text(), 'T478: missing production negotiation dependency')
            probe = output / 'app/src/main/java/NegotiatedInventory.kt'
            self.assertTrue(probe.exists())
            probe.unlink()
            (output / 'originals/MediaInventory.kt').unlink()
            project.copy_replay_sources(output, True)
            self.assertFalse(probe.exists(), 'T478: historical decoder cannot import newer inventory API')

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
