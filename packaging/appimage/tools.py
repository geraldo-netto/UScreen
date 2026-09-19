"""Pinned AppImage tool inputs; checksum verification precedes execution."""
import hashlib
import json
from pathlib import Path
import re
import tempfile
import urllib.request


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def validate(entry):
    if not isinstance(entry, dict) or not isinstance(entry.get('sha256'), str):
        raise ValueError('invalid tool metadata')
    if not re.fullmatch(r'[a-f0-9]{64}', entry['sha256']):
        raise ValueError('invalid tool digest')
    if not isinstance(entry.get('url'), str) or not entry['url'].startswith('https://github.com/AppImage/'):
        raise ValueError('tool URL must use the pinned AppImage organization')


def download(entry, target):
    validate(entry)
    fetch(entry, target)


def fetch(entry, target):
    """Transfer an already validated pinned input; never execute it here."""
    with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as temporary:
        path = Path(temporary.name)
        try:
            with urllib.request.urlopen(entry['url'], timeout=60) as response:
                transfer(response, temporary)
            temporary.flush()
            if digest(path) != entry['sha256']:
                raise ValueError('AppImage tool checksum mismatch')
            path.chmod(0o755)
            path.replace(target)
        finally:
            path.unlink(missing_ok=True)


def transfer(source, target):
    size = 0
    while block := source.read(65536):
        size += len(block)
        if size > 64 * 1024 * 1024:
            raise ValueError('AppImage tool download exceeds limit')
        target.write(block)


def prepare(cache):
    entries = json.loads(Path(__file__).with_name('tools.json').read_text())
    cache.mkdir(parents=True, exist_ok=True)
    paths = {}
    for name, entry in entries.items():
        validate(entry)
        target = cache / name
        if not target.is_file() or digest(target) != entry['sha256']:
            download(entry, target)
        target.chmod(0o755)
        paths[name] = target.resolve()
    return paths
