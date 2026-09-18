#!/usr/bin/env python3
"""Run an explicit decoder experiment plan, aborting on interruption or failure."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
from types import SimpleNamespace

SPEC = importlib.util.spec_from_file_location('decoder_device', Path(__file__).with_name('decoder-device.py'))
DEVICE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEVICE)


def validate_fields(row):
    profiles = {profile for _, profile in DEVICE.VARIANTS + DEVICE.INPUT_VARIANTS} | {'render-latest'}
    if row['profile'] not in profiles:
        raise ValueError('unknown replay profile')
    burst = row.get('burst', 1)
    if type(burst) is not int or not 1 <= burst <= 32:
        raise ValueError('invalid replay burst')
    for key, low, high in [('rate', 1, 90), ('seconds', 1, 600), ('warmup', 0, 60), ('trial', 0, 1000)]:
        if type(row[key]) is not int or not low <= row[key] <= high:
            raise ValueError(f'invalid replay {key}')


def validate(plan):
    seen = set()
    for row in plan:
        validate_fields(row)
        if not re.fullmatch(r'[a-z][a-z0-9-]*', row['scene']):
            raise ValueError('invalid scene name')
        identity = (row['scene'], row['rate'], row['trial'], row['profile'])
        if identity in seen:
            raise ValueError('duplicate trial identity')
        seen.add(identity)


def verify_apk(serial, provenance):
    package = provenance['package']
    paths = DEVICE.capture(serial, 'shell', 'pm', 'path', package).strip().splitlines()
    if len(paths) != 1 or not paths[0].startswith('package:/'):
        raise ValueError('expected one installed replay APK')
    path = paths[0].removeprefix('package:')
    digest = DEVICE.capture(serial, 'shell', 'sha256sum', path).split()[0]
    if digest != provenance['apk_sha256']:
        raise ValueError('installed replay APK differs from supplied provenance')


def run(args):
    plan = json.loads(args.plan.read_text())
    validate(plan)
    args.output.mkdir()
    provenance = json.loads(args.provenance.read_text())
    package = provenance['package']
    variant = package.removeprefix('com.uscreen.decoderbench.')
    if variant not in ['baseline', 'candidate']:
        raise ValueError('unexpected replay package')
    metadata = dict(plan=plan, plan_sha256=hashlib.sha256(args.plan.read_bytes()).hexdigest(),
                    provenance=provenance, serial=args.serial,
                    fingerprint=DEVICE.capture(args.serial, 'shell', 'getprop', 'ro.build.fingerprint').strip())
    verify_apk(args.serial, provenance)
    (args.output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    shutil.copy2(__file__, args.output / 'decoder-plan.py.txt')
    shutil.copy2(Path(__file__).with_name('decoder-device.py'), args.output / 'decoder-device.py.txt')
    for item in plan:
        fixture = (args.plan.parent / item['fixture']).resolve()
        trial = SimpleNamespace(serial=args.serial, output=args.output, seconds=item['seconds'], warmup=item['warmup'],
                                burst=item.get('burst', 1))
        setattr(trial, item['scene'], fixture)
        DEVICE.trial(trial, variant, item['profile'], item['scene'], item['rate'], item['trial'])
    DEVICE.capture(args.serial, 'shell', 'am', 'start', '-n', 'com.uscreen/.MainActivity')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--provenance', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
