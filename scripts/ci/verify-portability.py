#!/usr/bin/env python3
"""Check every shipped ELF's glibc floor and the replaceable helper library."""
from pathlib import Path
import re
import subprocess
import sys


def verify_abi(binary):
    symbols = subprocess.check_output(['objdump', '-T', str(binary)], text=True)
    versions = [tuple(map(int, version.split('.')))
                for version in re.findall(r'GLIBC_([0-9.]+)', symbols)]
    if not versions:
        raise ValueError(f'{binary}: no glibc version requirements found')
    required = max(versions)
    if required > (2, 36):
        raise ValueError(f'{binary}: requires glibc {required}, exceeds 2.36')
    print(f'{binary.name}: glibc {".".join(map(str, required))}')


def verify_bundle(folder):
    for name in ['blent', 'blent-gui', 'evdi_helper', 'libevdi.so.1.15.0']:
        verify_abi(folder / name)
    if (folder / 'libevdi.so.1').resolve() != folder / 'libevdi.so.1.15.0':
        raise ValueError('missing or incorrect bundled libevdi SONAME link')
    dynamic = subprocess.check_output(['readelf', '-d', str(folder / 'evdi_helper')], text=True)
    if not re.search(r'\((?:RUNPATH|RPATH)\).*\[\$ORIGIN\]', dynamic):
        raise ValueError('helper must load replaceable libevdi beside itself')
    if not re.search(r'\(NEEDED\).*\[libevdi\.so\.1\]', dynamic):
        raise ValueError('helper does not link the bundled libevdi SONAME')


if __name__ == '__main__':
    verify_bundle(Path(sys.argv[1]).resolve())
