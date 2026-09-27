#!/usr/bin/env python3
"""T652: repeatable, bounded native Rust mutation campaigns in private copies."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from evidence import summarize
from process import execute

ROOT = Path(__file__).resolve().parents[2]
VERSION = 'cargo-mutants 27.1.0'


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def tracked_paths(root):
    output = subprocess.check_output(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=root)
    return sorted(set(Path(os.fsdecode(name)) for name in output.split(b'\0') if name))


def fingerprint(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot(root, destination):
    destination.mkdir()
    hashes = {}
    for relative in tracked_paths(root):
        source = root / relative
        if source.is_symlink():
            raise ValueError(f'Source symlink requires explicit review: {relative}')
        if not source.exists():
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        hashes[relative.as_posix()] = fingerprint(target)
    return hashes


def unchanged(root, hashes):
    return all((root / name).is_file() and fingerprint(root / name) == value
               for name, value in hashes.items())


def profile(name, platform):
    profiles = json.loads(Path(__file__).with_name('profiles.json').read_text())
    selected = profiles[name]
    if platform not in selected['platforms']:
        raise ValueError(f'{name} requires native {selected["platforms"]}; got {platform}')
    return selected


def command(selected, args, output):
    result = ['cargo', 'mutants', '--no-config', '--baseline=run', '--copy-target=false',
              '--output', str(output), '--timeout', str(args.test_timeout),
              '--build-timeout', str(args.build_timeout), '--jobs', str(args.jobs),
              '--jobserver-tasks', str(args.build_jobs), '--cargo-arg=--locked', '--colors=never']
    for package in selected['packages']:
        result.extend(['--package', package])
    for package in selected.get('test_packages', []):
        result.extend(['--test-package', package])
        # 27.1.0 applies --test-package only to mutants, not the baseline.
        result.append('--cargo-arg=--package=' + package)
    for path in selected['files']:
        result.extend(['--file', path])
    result.extend('--cargo-arg=' + argument for argument in selected['cargo_args'])
    result.extend('--cargo-test-arg=' + argument for argument in selected.get('test_args', []))
    return result


def environment(output, build_jobs):
    value = dict(os.environ)
    for key in ['CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET', 'CARGO_MUTANTS_OUTPUT']:
        value.pop(key, None)
    scratch = output / 'scratch'
    scratch.mkdir()
    value.update(TMPDIR=str(scratch), TMP=str(scratch), TEMP=str(scratch),
                 CARGO_BUILD_JOBS=str(build_jobs), CARGO_TERM_COLOR='never')
    return value


def collect(output, process):
    try:
        return summarize(output / 'mutants.out', process)
    except (OSError, ValueError, KeyError, TypeError) as error:
        return dict(passes=False, complete=False, error=str(error),
                    outcomes={'tool_error': 1}, whole_project_mutation_score=None)


def run(args):
    selected = profile(args.profile, sys.platform)
    output = args.output.resolve()
    if output.is_relative_to(ROOT.resolve()):
        raise ValueError('Output must be outside the source checkout')
    output.mkdir(parents=True)
    version = subprocess.check_output(['cargo', 'mutants', '--version'], text=True).strip()
    if version != VERSION:
        raise ValueError(f'Requires {VERSION}; got {version}')
    source = output / 'source'
    hashes = snapshot(ROOT, source)
    invocation = command(selected, args, output)
    manifest = dict(source_commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'],
                    cwd=ROOT, text=True).strip(), sources=hashes, profile=args.profile,
                    selection=selected, platform=sys.platform, version=version, command=invocation)
    write_json(output / 'manifest.json', manifest)
    process = execute(invocation, source, environment(output, args.build_jobs),
                      output / 'run.log', args.campaign_timeout)
    report = collect(output, process)
    report.update(process=process, profile=args.profile, platform=sys.platform,
                  snapshot_unchanged=unchanged(source, hashes),
                  checkout_matches_snapshot=unchanged(ROOT, hashes))
    report['passes'] = report['passes'] and report['snapshot_unchanged']
    write_json(output / 'report.json', report)
    print(json.dumps(report, indent=2))
    return int(not report['passes'])


def positive(text):
    value = int(text)
    if value <= 0:
        raise argparse.ArgumentTypeError('Must be positive')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--jobs', type=positive, default=2)
    parser.add_argument('--build-jobs', type=positive, default=6)
    parser.add_argument('--test-timeout', type=positive, default=60)
    parser.add_argument('--build-timeout', type=positive, default=600)
    parser.add_argument('--campaign-timeout', type=positive, default=7200)
    return run(parser.parse_args())


if __name__ == '__main__':
    raise SystemExit(main())
