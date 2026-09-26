"""Copy an isolated conversion build, keeping every header from one revision."""
import hashlib
from pathlib import Path
import subprocess


def copy_sources(root, folder, reference=None):
    prefix = 'host/evdi/'
    if reference:
        names = subprocess.check_output(
            ['git', 'ls-tree', '--name-only', reference, prefix], cwd=root, text=True
        ).splitlines()
        headers = [Path(name).name for name in names if name.endswith('.h')]
    else:
        headers = [path.name for path in (root / prefix).glob('*.h')]
    sources = {}
    for name in sorted(set(['conversion.c', 'frame_exchange.c', *headers])):
        relative = prefix + name
        data = (subprocess.check_output(['git', 'show', f'{reference}:{relative}'], cwd=root)
                if reference else (root / relative).read_bytes())
        (folder / name).write_bytes(data)
        sources[relative] = hashlib.sha256(data).hexdigest()
    return sources


def build_flags(folder):
    header = (folder / 'frame_exchange.h').read_text()
    return ['-DBLENT_EXCHANGE_GENERATION'] if 'frame_exchange_begin(' in header else []
