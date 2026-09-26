"""Attest unchanged extracted Bash functions without storing script contents."""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile

from inventory import syntax_functions
from model import fingerprint, verify_sources
from readers import read_bounded


def signature(source, first, last):
    lines = source.splitlines(keepends=True)
    if not 1 <= first <= last <= len(lines):
        raise ValueError('invalid shell function bounds')
    return hashlib.sha256(''.join(lines[first - 1:last]).rstrip('\r\n').encode()).hexdigest()


def canonical(root, manifest):
    functions = [f for f in manifest['functions'] if f['language'] == 'shell']
    names = {f['file'] for f in functions}
    verify_sources(root, {name: manifest['sources'][name] for name in names})
    sources = {name: read_bounded(root/name).decode() for name in names}
    index = {}
    for function in functions:
        digest = signature(sources[function['file']], function['first'], function['last'])
        key = function['name'] + ':' + digest
        entry = {key: function[key] for key in ['file', 'name', 'first', 'last']}
        index.setdefault(key, []).append(dict(entry, body=digest))
    return {key: matches[0] for key, matches in index.items() if len(matches) == 1}


def matches(source, index):
    for function in syntax_functions(Path('runtime.sh'), source, 'shell'):
        digest = signature(source, function.first, function.last)
        original = index.get(function.name + ':' + digest)
        if original is not None:
            yield dict(original, at=function.first)


def atomic_json(path, value):
    with tempfile.NamedTemporaryFile(dir=path.parent, mode='w', delete=False) as stream:
        name = Path(stream.name)
        json.dump(value, stream)
        stream.write('\n')
    try:
        name.replace(path)
    finally:
        name.unlink(missing_ok=True)


def attested_index(directory, root, manifest_path):
    manifest_hash = fingerprint(manifest_path)
    path = directory/('index-' + manifest_hash + '.json')
    if not path.exists():
        manifest = json.loads(read_bounded(manifest_path))
        atomic_json(path, canonical(root, manifest))
    return json.loads(read_bounded(path))


def attest(path, digest, directory, root, manifest_path):
    if not re.fullmatch('[0-9a-f]{64}', digest):
        raise ValueError('invalid runtime shell digest')
    source = read_bounded(path)
    if hashlib.sha256(source).hexdigest() != digest:
        raise ValueError('runtime shell source changed while measuring')
    index = attested_index(directory, root, manifest_path)
    try:
        origins = list(matches(source.decode(), index))
    except UnicodeDecodeError:
        origins = []
    atomic_json(directory/(digest + '.json'), dict(digest=digest, functions=origins))


def main():
    directory = Path(os.environ['BLENT_SHELL_COVERAGE_DIR'])/'origins'
    directory.mkdir(parents=True, exist_ok=True)
    attest(Path(sys.argv[1]), sys.argv[2], directory,
           Path(os.environ['BLENT_COVERAGE_ROOT']), Path(os.environ['BLENT_COVERAGE_MANIFEST']))


if __name__ == '__main__':
    main()
