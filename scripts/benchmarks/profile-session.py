#!/usr/bin/env python3
"""T624: install a shell-only test APK for one workload, then remove it."""
import argparse
from contextlib import contextmanager
import importlib.util
from pathlib import Path
import re
import signal
import subprocess

PACKAGES = {'io.github.geraldo_netto.blent.profile', 'io.github.geraldo_netto.blent.optimized'}


def validate_apk(package, report):
    identities = re.findall(r"^package: name='([^']*)'", report, re.M)
    if package not in PACKAGES or identities != [package]:
        raise ValueError('APK must match an allowlisted disposable profiling package')
    if re.search(r'^launchable-activity:', report, re.M):
        raise ValueError('profiling APK must be shell-only; rebuild with the profile manifest')


def session(adb, package, apk, workload):
    if package not in PACKAGES:
        raise ValueError('refusing non-profiling package')
    installed = adb('shell', 'pm', 'list', 'packages', package).splitlines()
    if 'package:' + package in installed:
        raise ValueError('profiling package already installed; remove it explicitly first')
    try:
        adb('install', str(apk))
        workload()
    finally:
        adb('uninstall', package)


def terminate(signum, _frame):
    raise SystemExit(128 + signum)


@contextmanager
def interruptible():
    previous = signal.signal(signal.SIGTERM, terminate)
    try:
        yield
    finally:
        signal.signal(signal.SIGTERM, previous)


def run_workload(command):
    child = subprocess.Popen(command)
    try:
        code = child.wait()
        if code:
            raise subprocess.CalledProcessError(code, command)
    except BaseException:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
        raise


def apk_report(apk):
    verifier = Path(__file__).resolve().parents[1] / 'verify-release-apk.py'
    spec = importlib.util.spec_from_file_location('release_apk', verifier)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.output([module.build_tool('aapt2'), 'dump', 'badging', str(apk)])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--package', choices=sorted(PACKAGES), required=True)
    parser.add_argument('--apk', type=Path, required=True)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('supply a workload command after --')
    validate_apk(args.package, apk_report(args.apk))
    def adb(*parts):
        return subprocess.check_output(['adb', '-s', args.serial, *parts], text=True, timeout=120)
    with interruptible():
        session(adb, args.package, args.apk, lambda: run_workload(command))


if __name__ == '__main__':
    main()
