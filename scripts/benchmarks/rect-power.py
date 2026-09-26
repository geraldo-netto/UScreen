#!/usr/bin/env python3
"""T419: balanced USB-powered local replay comparison; never controls charging."""
import argparse
import json
from pathlib import Path
import subprocess
import time

CASES = [('text', 0, 5), ('text', 0, 1), ('text', 2, 5), ('pen', 0, 60), ('pen', 2, 60)]


def capture(serial, *command, **kwargs):
    return subprocess.run(['adb', '-s', serial, *command], check=True, timeout=30,
                          capture_output=True, text=True, **kwargs)


def power_config(seconds):
    if not 1 <= seconds <= 600:
        raise ValueError('replay duration outside bounded range')
    return '''buffers { size_kb: 4096 fill_policy: DISCARD }
data_sources { config { name: "android.power" android_power_config {
 battery_poll_ms: 5000
 battery_counters: BATTERY_COUNTER_CAPACITY_PERCENT
 battery_counters: BATTERY_COUNTER_CHARGE
 battery_counters: BATTERY_COUNTER_CURRENT
 battery_counters: BATTERY_COUNTER_VOLTAGE
} } }
duration_ms: %d
''' % ((seconds + 90) * 1000)


def stop_trace(serial, pid):
    capture(serial, 'shell', 'kill', '-INT', str(pid))
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        alive = subprocess.run(['adb', '-s', serial, 'shell', 'kill', '-0', str(pid)],
                               capture_output=True, timeout=5)
        if alive.returncode:
            return
        time.sleep(.2)
    raise TimeoutError('Perfetto did not finish flushing its trace')


def case(args, index, specification):
    scene, codec, rate = specification
    folder = args.output / f'{index:02d}-{scene}-{codec}-{rate}'
    folder.mkdir()
    config = power_config(args.seconds)
    (folder / 'power.pbtxt').write_text(config)
    remote = f'/data/misc/perfetto-traces/blent-t419-power-{index:02d}.pftrace'
    started = capture(args.serial, 'shell', 'perfetto', '--txt', '--background-wait',
                      '-c', '-', '-o', remote, input=config)
    pid = int(started.stdout.strip())
    (folder / 'trace-start.json').write_text(json.dumps(dict(pid=pid, stdout=started.stdout, stderr=started.stderr)))
    command = ['python3', str(Path(__file__).with_name('rect-device.py')),
               '--serial', args.serial, '--fixtures', str(args.fixtures), '--provenance', str(args.provenance),
               '--output', str(folder / 'replay'), '--scenes', scene, '--codecs', str(codec),
               '--seconds', str(args.seconds), '--warmup', '4', '--repeats', '1', '--sample-period', '30']
    if rate == 1:
        command += ['--text-rate', '1']
    (folder / 'command.json').write_text(json.dumps(command, indent=2) + '\n')
    try:
        with (folder / 'replay.log').open('w') as log:
            subprocess.run(command, check=True, timeout=args.seconds + 75, stdout=log, stderr=subprocess.STDOUT)
    finally:
        stop_trace(args.serial, pid)
        capture(args.serial, 'pull', remote, str(folder / 'power.pftrace'))
        capture(args.serial, 'shell', 'rm', remote)
    print(index, specification, 'complete', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--provenance', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seconds', type=int, choices=range(1, 601), default=480)
    args = parser.parse_args()
    args.output.mkdir()
    order = CASES + list(reversed(CASES))
    (args.output / 'plan.json').write_text(json.dumps(dict(seconds=args.seconds, order=order,
        boundary='Local files; USB remains plugged in. Signed battery current is net battery flow, not total device power.'), indent=2) + '\n')
    for index, specification in enumerate(order):
        case(args, index, specification)


if __name__ == '__main__':
    main()
