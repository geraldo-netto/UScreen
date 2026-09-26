#!/usr/bin/env python3
"""T497: preserve GCC counters from temporary normal-suite C executables."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def compilation(arguments):
    sources = [Path(arg) for arg in arguments if arg.endswith('.c') and Path(arg).is_file()]
    if not sources or '-o' not in arguments:
        return None
    index = arguments.index('-o') + 1
    if index >= len(arguments):
        return None
    return Path(arguments[index]).absolute()


def capture_notes(output, destination):
    prefix = output.name.removesuffix('.o')
    notes = list(output.parent.glob(prefix + '*.gcno'))
    if not notes:
        raise ValueError(f'compiler produced no coverage notes for {output}')
    for note in notes:
        shutil.copy2(note, destination / note.name)
    return [note.name for note in notes]


def owned_compilation(arguments, root):
    directories = [root / name for name in ['host/evdi', 'host/tests', 'scripts/benchmarks']]
    sources = [Path(arg).resolve() for arg in arguments if arg.endswith('.c') and Path(arg).is_file()]
    return any(source.is_relative_to(directory) for source in sources for directory in directories)


def compile_with_profile(compiler, arguments, report, root=None):
    output = compilation(arguments)
    if output is None or (root is not None and not owned_compilation(arguments, root)):
        return subprocess.run([compiler, *arguments], check=False).returncode
    report.mkdir(parents=True, exist_ok=True)
    destination = Path(tempfile.mkdtemp(prefix='c-', dir=report))
    command = [compiler, *arguments, '--coverage', '-fprofile-update=atomic',
               '-fprofile-dir=' + str(destination / 'counters')]
    result = subprocess.run(command, check=False)
    if result.returncode:
        return result.returncode
    notes = capture_notes(output, destination)
    (destination / 'compile.json').write_text(json.dumps(dict(command=command, notes=notes)) + '\n')
    return 0


def pair_counter(note, counters):
    suffix = note.with_suffix('.gcda').name
    matches = [path for path in counters.rglob('*.gcda') if path.name.endswith(suffix)]
    if len(matches) > 1:
        raise ValueError(f'ambiguous counter match: {note}')
    if matches:
        shutil.copy2(matches[0], note.with_suffix('.gcda'))


def export_reports(report):
    outputs = []
    for directory in sorted(report.glob('c-*')):
        notes = list(directory.glob('*.gcno'))
        if not notes:
            continue
        for note in notes:
            pair_counter(note, directory / 'counters')
        subprocess.run(['gcov', '--json-format', *map(str, notes)], cwd=directory, check=True,
                       stdout=subprocess.DEVNULL)
        outputs.extend(directory.glob('*.gcov.json.gz'))
    return sorted(outputs)


def main():
    report = Path(os.environ['BLENT_C_COVERAGE']).resolve()
    compiler = os.environ['BLENT_REAL_CC']
    if Path(compiler).resolve() == Path(sys.argv[0]).resolve():
        raise ValueError('coverage compiler cannot recursively invoke itself')
    return compile_with_profile(compiler, sys.argv[1:], report, Path(__file__).resolve().parents[2])


if __name__ == '__main__':
    raise SystemExit(main())
