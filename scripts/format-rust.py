#!/usr/bin/env python3
"""Format/check all Rust source roots, including the Linux supervisor include."""
import argparse
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    flags = ['--check'] if args.check else []
    commands = [
        ['cargo', 'fmt', '--all', '--', *flags],
        ['rustfmt', '--edition', '2021', 'host/src/linux_main.rs', *flags],
    ]
    for command in commands:
        result = subprocess.run(command, cwd=ROOT, check=False)
        if result.returncode:
            return result.returncode
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
