"""Native Bash location events matched to byte-identical maintained sources."""
from pathlib import Path
import json
import re

from inventory import parse, walk
from model import add_line
from readers import read_bounded
from shell_origins import canonical

EXECUTABLE = {'command', 'variable_assignment', 'declaration_command', 'test_command',
              'for_statement', 'while_statement', 'case_statement'}


def executable_lines(source):
    return {node.start_point.row + 1 for node in walk(parse(source, 'shell')) if node.type in EXECUTABLE}


def records(path):
    fields = read_bounded(path).split(b'\0')
    if fields.pop() != b'' or len(fields) % 3:
        raise ValueError(f'truncated Bash coverage record: {path}')
    for index in range(0, len(fields), 3):
        digest, name, line = fields[index:index + 3]
        if not re.fullmatch(b'[0-9a-f]{64}', digest) or not re.fullmatch(b'[1-9][0-9]*', line):
            raise ValueError('invalid Bash source hash or location')
        yield digest.decode(), name.decode(), int(line)


def seed(root, manifest, data):
    sources = {}
    files = {function['file'] for function in manifest['functions'] if function['language'] == 'shell'}
    for name in files:
        sources.setdefault(manifest['sources'][name], []).append(name)
        for line in executable_lines((root / name).read_text()):
            add_line(data, name, line, 0)
    return sources


def origins(root, directory, manifest):
    if (directory/'origins.failed').exists():
        raise ValueError('Bash origin attestation failed')
    expected = canonical(root, manifest)
    result = {}
    for path in (directory/'origins').glob('*.json'):
        if path.name.startswith('index-'):
            continue
        record = json.loads(read_bounded(path))
        digest = record['digest']
        if not re.fullmatch('[0-9a-f]{64}', digest) or path.stem != digest:
            raise ValueError('invalid Bash origin digest')
        result[digest] = [validate_origin(row, expected) for row in record['functions']]
    return result


def validate_origin(row, expected):
    at = row.get('at')
    if type(at) is not int or not 1 <= at <= (1 << 31) - 1:
        raise ValueError('invalid extracted Bash function location')
    original = {key: value for key, value in row.items() if key != 'at'}
    if original != expected.get(row['name'] + ':' + row['body']):
        raise ValueError('unrecognized extracted Bash function')
    return row


def credit_origins(data, rows, line):
    for row in rows:
        original_line = line - row['at'] + row['first']
        if row['first'] <= original_line <= row['last'] and original_line in data.get(row['file'], {}):
            add_line(data, row['file'], original_line, 1)


def merge(root, directory, manifest, data):
    sources = seed(root, manifest, data)
    mapped = origins(root, directory, manifest)
    for path in directory.glob('trace-*.bin'):
        for digest, _, line in records(path):
            names = sources.get(digest, [])
            if len(names) == 1 and line in data.get(names[0], {}):
                add_line(data, names[0], line, 1)
            else:
                credit_origins(data, mapped.get(digest, []), line)
