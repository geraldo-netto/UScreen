"""T442: sparse observations and recorded plot provenance."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location('baseline_plot', SCRIPTS / 'plot-baseline.py')
PLOT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PLOT)


class PlotTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.folder = Path(directory.name)
        self.meta = dict(source_commit='aabbccddeeff', geometry='800x600+100+100', start_utc=0)
        (self.folder / 'metadata.json').write_text(json.dumps(self.meta))
        for name in ('samples', 'phases', 'host-windows'):
            (self.folder / f'{name}.jsonl').write_text('')

    def test_t442_empty_observations_preserve_empty_series(self):
        meta, charge, cpu, latency = PLOT.series(self.folder)
        self.assertEqual(meta, self.meta)
        self.assertEqual((charge, cpu, latency), ([], [], []))

    def test_t457_plot_accepts_current_and_historical_latency_labels(self):
        rows = [dict(utc=60, message='Latency encode→display: p50 5ms  p95 6ms  max 7ms  (3 samples)'),
                dict(utc=120, message='Latency packet-ready→render-ACK (host clock): p50 8ms  p95 9ms  max 10ms  (3 samples)')]
        (self.folder / 'host-windows.jsonl').write_text(''.join(json.dumps(row) + '\n' for row in rows))
        self.assertEqual(PLOT.series(self.folder)[3], [(1, 5, 6), (2, 8, 9)])

    def render(self, rows):
        matplotlib = MagicMock()
        figure, axes = MagicMock(), [MagicMock() for _ in range(3)]
        matplotlib.pyplot.subplots.return_value = figure, axes
        with patch.dict(sys.modules, {'matplotlib': matplotlib, 'matplotlib.pyplot': matplotlib.pyplot}), \
             patch.object(PLOT, 'series', return_value=(self.meta, *rows)):
            PLOT.plot(self.folder, self.folder / 'output.png')
        figure.savefig.assert_called_once()
        matplotlib.pyplot.close.assert_called_once_with(figure)
        return figure, axes

    def test_t442_missing_traces_are_labelled(self):
        _, axes = self.render(([], [], []))
        for axis in axes:
            axis.text.assert_called_once()
            self.assertIn('No observations', axis.text.call_args.args[2])

    def test_t442_title_uses_only_recorded_settings(self):
        figure, _ = self.render(([(0, 0)], [(0, 5)], [(0, 1, 2)]))
        title = figure.suptitle.call_args.args[0]
        self.assertIn('800x600+100+100', title)
        self.assertIn('unverified', title)
        self.assertNotIn('VAAPI', title)
        self.assertNotIn('60 fps', title)

    def test_t442_unknown_pipeline_cpu_is_not_an_observation(self):
        _, axes = self.render(([], [(0, float('nan'))], []))
        axes[1].text.assert_called_once()
        axes[1].plot.assert_not_called()


if __name__ == '__main__':
    unittest.main()
