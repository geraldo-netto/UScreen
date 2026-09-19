"""Use the same pinned Kotlin compiler as the cyclomatic-complexity inventory."""
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'complexity'))
from kotlin import ARTIFACTS, artifact, cache_dir
from model import Function, fingerprint


def functions(root, paths):
    if not paths:
        return []
    cache = cache_dir()
    classpath = os.pathsep.join(artifact(cache, key, digest) for key, digest in ARTIFACTS.items())
    source = Path(__file__).with_name('KotlinFunctions.kt')
    jar = cache / f'functions-{fingerprint(source)}.jar'
    if not jar.exists():
        subprocess.run(['java', '-cp', classpath, 'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler',
                        '-no-stdlib', '-no-reflect', '-cp', classpath, '-d', str(jar), str(source)], check=True)
    output = subprocess.check_output(['java', '-Djava.awt.headless=true', '-cp',
                                      f'{jar}{os.pathsep}{classpath}', 'KotlinFunctionsKt',
                                      *map(str, paths)], text=True)
    return [parse_result(root, line) for line in output.splitlines()]


def parse_result(root, line):
    path, first, last, name, body = line.split('\t')
    return Function(str(Path(path).resolve().relative_to(root.resolve())), name, int(first), int(last), 'kotlin', body=int(body))
