"""T419: battery-current sign, clocks and observation coverage are evidence contracts."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1] / 'benchmarks'
sys.path.insert(0, str(ROOT))
SPEC = importlib.util.spec_from_file_location('rect_power_report', ROOT / 'rect-power-report.py')
POWER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POWER)


class RectanglePowerTests(unittest.TestCase):
    def test_t419_missing_or_invalid_counters_cannot_become_zero_consumption(self):
        row = self.samples()[0]
        names = {name for name in row if name.startswith('batt.')}
        POWER.validate_counters(row, names)
        for changed in [dict(row, **{'batt.current_ua': float('nan')}),
                        dict(row, **{'batt.charge_uah': -1}),
                        {'batt.current_ua': 0}]:
            with self.assertRaises(ValueError):
                POWER.validate_counters(changed, names)

    def samples(self, current=-360_000):
        return [dict(ts=i * 5_000_000_000, **{'batt.current_ua': current,
                     'batt.charge_uah': 100_000 - i * 500, 'batt.voltage_uv': 4_000_000,
                     'batt.capacity_pct': 50}) for i in range(4)]

    def test_t419_signed_flow_keeps_discharge_negative(self):
        row = POWER.battery_summary(self.samples(), 0, 15_000_000_000)
        self.assertEqual(row['mean_current_ma'], -360)
        self.assertEqual(row['integrated_net_charge_mah'], -1.5)
        self.assertEqual(row['gauge_net_charge_mah'], -1.5)
        charged = POWER.battery_summary(self.samples(360_000), 0, 15_000_000_000)
        self.assertEqual(charged['integrated_net_charge_mah'], 1.5)

    def test_t419_missing_or_repeated_time_is_not_interpolated_as_a_good_run(self):
        samples = self.samples()
        with self.assertRaises(ValueError):
            POWER.battery_summary(samples[1:], 0, 25_000_000_000)
        with self.assertRaises(ValueError):
            POWER.battery_summary([samples[0], samples[2], samples[3]], 0, 15_000_000_000)
        with self.assertRaises(ValueError):
            POWER.integrate([0, 0], [1, 2])

    def test_t419_charge_counter_quantization_is_retained(self):
        samples = self.samples()
        for sample in samples:
            sample['batt.charge_uah'] = 99_900
        row = POWER.battery_summary(samples, 0, 15_000_000_000)
        self.assertEqual(row['gauge_net_charge_mah'], 0)
        self.assertNotEqual(row['integrated_net_charge_mah'], 0)
        self.assertEqual(row['nonzero_gauge_steps_uah'], [])


if __name__ == '__main__':
    unittest.main()
