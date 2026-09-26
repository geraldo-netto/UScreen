#!/usr/bin/env python3
"""Fail closed before bundling/publishing the official fork Android APK (T250)."""
import argparse
import base64
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = 'io.github.geraldo_netto.blent'
MAX_OUTPUT = 1024 * 1024


def certificate_digest(path):
    text = path.read_text(encoding='ascii')
    match = re.fullmatch(r'-----BEGIN CERTIFICATE-----\s+([A-Za-z0-9+/=\s]+)-----END CERTIFICATE-----\s*', text)
    if len(text) > 65536 or match is None:
        raise ValueError('invalid designated public certificate')
    der = base64.b64decode(''.join(match[1].split()), validate=True)
    if not der:
        raise ValueError('empty designated public certificate')
    return hashlib.sha256(der).hexdigest()


def sdk_root():
    for name in ('ANDROID_SDK_ROOT', 'ANDROID_HOME'):
        if os.environ.get(name):
            return Path(os.environ[name])
    props = ROOT / 'android/local.properties'
    if props.is_file():
        for line in props.read_text().splitlines():
            if line.startswith('sdk.dir='):
                return Path(line.removeprefix('sdk.dir='))
    raise ValueError('Android SDK missing; set ANDROID_SDK_ROOT')


def build_tool(name):
    found = shutil.which(name)
    if found:
        return found
    choices = list((sdk_root() / 'build-tools').glob(f'[0-9]*/{name}'))
    choices = [p for p in choices if re.fullmatch(r'\d+\.\d+\.\d+', p.parent.name)]
    choices.sort(key=lambda p: tuple(map(int, p.parent.name.split('.'))))
    if not choices:
        raise ValueError(f'Android build tool missing: {name}')
    return str(choices[-1])


def output(command):
    result = subprocess.run(command, capture_output=True, text=True, timeout=60, check=True)
    if len(result.stdout) > MAX_OUTPUT:
        raise ValueError('Android tool output exceeds limit')
    return result.stdout


def verify_signer(text, expected):
    if len(text) > MAX_OUTPUT:
        raise ValueError('signer report exceeds limit')
    digests = re.findall(r'^Signer #\d+ certificate SHA-256 digest: (.*)$', text, re.M)
    if len(digests) != 1 or re.fullmatch(r'[0-9A-Fa-f]{64}', digests[0]) is None:
        raise ValueError('APK must have exactly one valid signing certificate')
    if digests[0].lower() != expected:
        raise ValueError('APK certificate does not match designated fork release key')


def verify_manifest(text):
    if len(text) > MAX_OUTPUT:
        raise ValueError('manifest report exceeds limit')
    packages = re.findall(r"^package: name='([^']*)'", text, re.M)
    activities = re.findall(r"^launchable-activity: name='([^']*)'", text, re.M)
    if packages != [PACKAGE] or activities != ['com.blent.MainActivity']:
        raise ValueError('APK package or launcher does not match fork identity')
    if re.search(r'^application-debuggable(?:\s|$)', text, re.M):
        raise ValueError('official release APK must not be debuggable')


def verify(apk):
    expected = certificate_digest(ROOT / 'docs/release-certificate.pem')
    verify_signer(output([build_tool('apksigner'), 'verify', '--verbose', '--print-certs', str(apk)]), expected)
    verify_manifest(output([build_tool('aapt2'), 'dump', 'badging', str(apk)]))
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('apk', type=Path)
    args = parser.parse_args()
    try:
        digest = verify(args.apk)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f'APK verification failed: {error}\n')
    print(f'Verified {PACKAGE}; certificate SHA-256 {digest}')


if __name__ == '__main__':
    main()
