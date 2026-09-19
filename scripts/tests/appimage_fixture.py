"""Offline package orchestration fixture; real ELF closure has separate tests."""
from pathlib import Path


def install(root):
    builder = root / 'packaging/appimage/build.py'
    builder.parent.mkdir(parents=True, exist_ok=True)
    builder.write_text('''from pathlib import Path
import argparse, os, shutil
p = argparse.ArgumentParser()
p.add_argument('--bundle'); p.add_argument('--output'); p.add_argument('--version'); p.add_argument('--evdi-source')
a = p.parse_args()
if os.environ.get('USCREEN_TEST_MODE') == 'appimage': raise SystemExit(42)
out = Path(a.output)
for suffix in ('x86_64.AppImage', 'AppImage-sources.tar.gz'):
    (out / f'uscreen-{a.version}-{suffix}').write_text('new')
source = Path(a.bundle)
target = out / 'appimage-docs-fixture'
if target.exists(): shutil.rmtree(target)
shutil.copytree(source, target)
''')
