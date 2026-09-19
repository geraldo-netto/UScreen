#!/usr/bin/env python3
"""T381: fail closed on parse errors and functions over nine."""
import argparse
from pathlib import Path
import re
import subprocess
import sys
from kotlin import kotlin_scores
from syntax import embedded_python, python_scores, syntax_scores

ROOT = Path(__file__).resolve().parents[2]
LANGUAGES = {'.rs': 'rust', '.c': 'c', '.h': 'c', '.py': 'python',
             '.sh': 'shell', '.install': 'shell', '.kt': 'kotlin', '.kts': 'kotlin', '.spec': 'rpm'}
EXCLUDED = {'host/evdi/evdi_lib.h', 'android/gradlew', 'android/gradlew.bat'}
GENERATED = {'build', 'dist', 'target', 'target-deb12', 'target-portability', 'target-appimage-tools', 'target-appimage-sources',
             '.gradle', '.git', '.cache', '__pycache__', 'node_modules', '.venv', 'vendor'}


def language(path):
    if path.name in {'PKGBUILD', 'postinst'}:
        return 'shell'
    return LANGUAGES.get(path.suffix)


def owned(path):
    return path.as_posix() not in EXCLUDED and not GENERATED.intersection(path.parts)


def source_files(root):
    output = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others',
                                      '--exclude-standard'], cwd=root)
    paths = {Path(name.decode()) for name in output.split(b'\0') if name}
    return sorted(path for path in paths if owned(path) and language(path) and (root / path).is_file())


def file_scores(path):
    source = path.read_text()
    kind = language(path)
    if kind == 'python':
        yield from python_scores(source)
        return
    if kind == 'rpm':
        for offset, script in rpm_sections(source):
            for line, name, score in syntax_scores(script, 'shell'):
                yield offset + line, name, score
        return
    yield from syntax_scores(source, kind)
    if kind == 'shell':
        for offset, script in embedded_python(source):
            for line, name, score in python_scores(script):
                yield offset + line, name, score


def rpm_sections(source):
    pattern = r'^%(?:prep|build|install|check|pre|post|preun|postun|pretrans|posttrans)(?:[ \t].*)?\n'
    for match in re.finditer(pattern, source, flags=re.M):
        rest = source[match.end():]
        end = re.search(r'^%(?:prep|build|install|check|pre|post|preun|postun|pretrans|posttrans|files|description|changelog|package)\b', rest, flags=re.M)
        script = rest[:end.start()] if end else rest
        yield source[:match.end()].count('\n'), script


def collect(root, paths):
    kotlin = []
    for path in paths:
        if language(path) == 'kotlin':
            kotlin.append(root / path)
            continue
        try:
            for line, name, score in file_scores(root / path):
                yield str(path), line, name, score
        except (ValueError, SyntaxError) as error:
            raise ValueError(f'{path}: {error}') from error
    for path, line, name, score in kotlin_scores(kotlin):
        yield str(Path(path).relative_to(root)), line, name, score


def report(rows, verbose=False):
    failures = 0
    for path, line, name, score in rows:
        if verbose or score > 9:
            print(f'{path}:{line}\t{name}\t{score}')
        failures += score > 9
    print(f'{len(rows)} functions; {failures} exceed cyclomatic complexity 9')
    return int(failures > 0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verbose', action='store_true')
    args = parser.parse_args()
    try:
        return report(list(collect(ROOT, source_files(ROOT))), args.verbose)
    except (ValueError, SyntaxError, OSError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
