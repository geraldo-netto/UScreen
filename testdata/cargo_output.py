#!/usr/bin/env python3
"""Disposable Cargo fixture: honor target-dir precedence and write identifiable outputs."""
from pathlib import Path
import json
import os
import shutil
import sys
import tomllib


def target_directory(args):
    if '--target-dir' in args:
        return Path(args[args.index('--target-dir') + 1])
    if os.environ.get('CARGO_TARGET_DIR'):
        return Path(os.environ['CARGO_TARGET_DIR'])
    config = Path('.cargo/config.toml')
    if config.exists():
        return Path(tomllib.loads(config.read_text())['build']['target-dir'])
    return Path('target')


def main():
    args = sys.argv[1:]
    target = target_directory(args)
    with open(os.environ['BLENT_T336_CARGO_LOG'], 'a') as log:
        log.write(json.dumps([str(target), args]) + '\n')
    if args[0] == 'clean':
        shutil.rmtree(target, ignore_errors=True)
        return
    output = target / 'release'
    output.mkdir(parents=True, exist_ok=True)
    for name in ['blent', 'blent-gui']:
        program = output / name
        program.write_text('#!/bin/sh\nprintf \'fresh-' + name + ' %s\\n\' "${1:-}"\n')
        program.chmod(0o755)


if __name__ == '__main__':
    main()
