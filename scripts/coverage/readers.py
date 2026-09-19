"""Read native coverage counters, preserving zeros and rejecting malformed values."""
import gzip
import json
from pathlib import Path
import xml.etree.ElementTree as ET

from model import add_line, source_path, validate_count

LIMIT = 64 * 1024 * 1024


def read_bounded(path, compressed=False):
    opener = gzip.open if compressed else open
    with opener(path, 'rb') as stream:
        data = stream.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError('coverage input exceeds 64 MiB')
    return data


def lcov(path, root, data, prefix=None):
    current = None
    for line in read_bounded(path).decode().splitlines():
        if line.startswith('SF:'):
            current = source_path(root, line[3:], prefix)
        elif line.startswith('DA:') and current is not None:
            number, count, *_ = line[3:].split(',')
            add_line(data, current, int(number), int(count))
        elif line == 'end_of_record':
            current = None


def python_json(path, root, data, prefix=None):
    report = json.loads(read_bounded(path))
    for name, record in report['files'].items():
        normalized = source_path(root, name, prefix)
        if normalized is None:
            continue
        for line in record['missing_lines']:
            add_line(data, normalized, line, 0)
        for line in record['executed_lines']:
            add_line(data, normalized, line, 1)


def gcov_json(path, root, data, prefix=None):
    report = json.loads(read_bounded(path, compressed=True))
    base = Path(report['current_working_directory'])
    for record in report['files']:
        normalized = source_path(root, str(base / record['file']), prefix)
        if normalized is None:
            continue
        for line in record['lines']:
            add_line(data, normalized, line['line_number'], line['count'])


def jacoco(path, source_root, root, data):
    report = ET.fromstring(read_bounded(path))
    for package in report.findall('package'):
        directory = source_root / package.attrib['name']
        for source in package.findall('sourcefile'):
            normalized = source_path(root, str(directory / source.attrib['name']))
            if normalized is None:
                continue
            for line in source.findall('line'):
                covered = validate_count(int(line.attrib['ci']))
                missed = validate_count(int(line.attrib['mi']))
                if covered + missed:
                    add_line(data, normalized, int(line.attrib['nr']), covered)
