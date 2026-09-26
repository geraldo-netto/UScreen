#!/usr/bin/env python3
"""Run T608 trials in disposable user/network namespaces; never alter host routes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time


def run(args):
    args.output.mkdir(parents=True)
    script = Path(__file__).with_name('camera-transport.py')
    parent = os.readlink('/proc/self/ns/net')
    records = []
    for trial in range(args.trials):
        for scenario in args.scenarios:
            protocols = ['tcp', 'udp'] if trial % 2 == 0 else ['udp', 'tcp']
            for protocol in protocols:
                path = args.output / f'{trial}-{scenario}-{protocol}.json'
                command = ['unshare', '--user', '--map-root-user', '--net', 'python3', str(script),
                           '--fixture', str(args.fixture), '--output', str(path), '--protocol', protocol,
                           '--scenario', scenario, '--parent-netns', parent]
                subprocess.run(command, check=True, timeout=45)
                row = json.loads(path.read_text())
                row['trial'] = trial
                records.append(row)
    metadata = dict(kernel=platform.platform(), fixture_sha256=hashlib.sha256(args.fixture.read_bytes()).hexdigest(),
                    trials=args.trials, timestamp=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                    netem_seed='unsupported by installed iproute2; interleaved independent random trials',
                    sources={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [*script.parent.glob('camera*transport*.py'), script.parent / 'camera_datagram.py']})
    (args.output / 'results.json').write_text(json.dumps(dict(metadata=metadata, rows=records), indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, choices=range(1, 4), default=3)
    parser.add_argument('--scenarios', nargs='+', choices=['clean', 'loss-jitter', 'pressure'], default=['clean', 'loss-jitter', 'pressure'])
    run(parser.parse_args())


if __name__ == '__main__':
    main()
