#!/usr/bin/env python3
"""T410: recover original timestamped baseline windows from retained journald data."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import time

from observe import journal_record


def recover(folder):
    meta = json.loads((folder / 'metadata.json').read_text())
    events = [json.loads(line) for line in (folder / 'phases.jsonl').read_text().splitlines()]
    end = events[-1]['utc'] if events[-1]['event'] == 'complete' else time.time()
    args = ['journalctl', '--user', '-u', 'uscreen', '-o', 'json', '--no-pager',
            '--since', f'@{meta["start_utc"]}', '--until', f'@{end}']
    result = subprocess.run(args, text=True, capture_output=True, check=True, timeout=30)
    pattern = re.compile(r'Latency |of which tablet|Encoder: \d|evdi-helper.*(grabs/s|cycle:|capture|Incomplete|Mode:)|FIFO_RESET|Client lagged|Capture manager failed')
    count = 0
    with (folder / 'host-windows-recovered.jsonl').open('w') as output:
        for line in result.stdout.splitlines():
            record = journal_record(line)
            if pattern.search(record['message']):
                output.write(re.sub(r'\b[0-9a-fA-F]{64}\b', '[redacted]', json.dumps(record)) + '\n')
                count += 1
    print(f'Recovered {count} original performance messages through {end}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('folder', type=Path)
    recover(parser.parse_args().folder)
