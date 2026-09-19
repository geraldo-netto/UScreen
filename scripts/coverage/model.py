"""Strict source-line coverage contracts for T497; missing evidence never passes."""
from dataclasses import dataclass
from pathlib import Path
import hashlib


@dataclass(frozen=True)
class Function:
    file: str
    name: str
    first: int
    last: int
    language: str
    nested: tuple = ()
    entry: int = 0
    body: int = 0


def source_path(root, name, prefix=None):
    root = root.resolve()
    path = Path(name)
    if prefix is not None and path.is_absolute():
        try:
            path = path.relative_to(prefix)
        except ValueError:
            return None
    if not path.is_absolute():
        path = root / path
    try:
        return path.resolve().relative_to(root).as_posix()
    except ValueError:
        return None


def fingerprint(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def validate_count(value):
    if type(value) is not int or value < 0 or value > (1 << 63) - 1:
        raise ValueError('coverage count must be a nonnegative signed 64-bit integer')
    return value


def add_line(data, file, line, count):
    validate_count(line)
    validate_count(count)
    if line == 0:
        raise ValueError('coverage line numbers start at one')
    current = data.setdefault(file, {})
    current[line] = max(current.get(line, 0), count)


def result(function, data):
    lines = {line: count for line, count in data.get(function.file, {}).items()
             if (function.body or function.first) <= line <= function.last
             and not any(first <= line <= last for first, last in function.nested)}
    covered = sum(count > 0 for count in lines.values())
    total = len(lines)
    return dict(file=function.file, name=function.name, line=function.first,
                language=function.language, covered=covered, total=total,
                percent=100 * covered / total if total else None,
                passes=bool(total) and covered * 5 >= total * 4,
                missing=[line for line, count in sorted(lines.items()) if count == 0])


def verify_sources(root, expected):
    for name, checksum in expected.items():
        normalized = source_path(root, name)
        if normalized != name or fingerprint(root / name) != checksum:
            raise ValueError(f'coverage source changed or escaped the project: {name}')
