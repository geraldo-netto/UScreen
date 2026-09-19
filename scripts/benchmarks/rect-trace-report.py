#!/usr/bin/env python3
"""T419: diagnose one complete replay/Perfetto trace without changing old summaries."""
import argparse
import json
from pathlib import Path
from rect_trace import diagnose


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--processor', required=True)
    parser.add_argument('--trace', required=True, type=Path)
    parser.add_argument('--trial', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = json.loads((args.trial / 'result.json').read_text())
    if not result.get('completed') or result.get('mode') == 'rect':
        raise ValueError('requires a completed hardware-video replay')
    records = [json.loads(line) for line in (args.trial / 'surface.jsonl').read_text().splitlines()]
    report = diagnose(args.trace, args.processor, result, records)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print({name: len(rows) for name, rows in report['classification'].items()})


if __name__ == '__main__':
    main()
