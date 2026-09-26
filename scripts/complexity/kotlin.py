"""Pinned Kotlin PSI compiler, cached outside the source tree and hash verified."""
import hashlib
import os
from pathlib import Path
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parent
ARTIFACTS = {
    'org/jetbrains/kotlin/kotlin-compiler-embeddable/1.9.20/kotlin-compiler-embeddable-1.9.20.jar':
        'a25024fe5da8440de01af045c4fcb954a22f078738ec02616085f0cfc57b2702',
    'org/jetbrains/kotlin/kotlin-stdlib/1.9.20/kotlin-stdlib-1.9.20.jar':
        '28a35bcdff46d864f80f346a617e486284b208d17378c41900dfb1de95a90e6c',
    'org/jetbrains/intellij/deps/trove4j/1.0.20200330/trove4j-1.0.20200330.jar':
        'c5fd725bffab51846bf3c77db1383c60aaaebfe1b7fe2f00d23fe1b7df0a439d',
    'org/jetbrains/annotations/13.0/annotations-13.0.jar':
        'ace2a10dc8e2d5fd34925ecac03e4988b2c0f851650c94b8cef49ba1bd111478',
}


def cache_dir():
    base = Path(os.environ.get('XDG_CACHE_HOME') or Path.home() / '.cache')
    result = base / 'blent-complexity'
    result.mkdir(parents=True, exist_ok=True)
    return result


def artifact(cache, coordinate, digest):
    path = cache / coordinate.rsplit('/', 1)[-1]
    if not path.exists():
        with urllib.request.urlopen('https://repo.maven.apache.org/maven2/' + coordinate, timeout=60) as response:
            content = response.read()
        if hashlib.sha256(content).hexdigest() != digest:
            raise ValueError(f'Kotlin dependency checksum mismatch: {coordinate}')
        path.write_bytes(content)
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        raise ValueError(f'Kotlin cache checksum mismatch: {path}')
    return str(path)


def kotlin_scores(paths):
    if not paths:
        return []
    cache = cache_dir()
    classpath = os.pathsep.join(artifact(cache, key, digest) for key, digest in ARTIFACTS.items())
    source = ROOT / 'KotlinMetrics.kt'
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    jar = cache / f'metrics-{digest}.jar'
    if not jar.exists():
        subprocess.run(['java', '-cp', classpath, 'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler',
                        '-no-stdlib', '-no-reflect', '-cp', classpath, '-d', str(jar), str(source)], check=True)
    output = subprocess.check_output(['java', '-Djava.awt.headless=true', '-cp',
                                      f'{jar}{os.pathsep}{classpath}', 'KotlinMetricsKt', *map(str, paths)], text=True)
    return [parse_result(line) for line in output.splitlines()]


def parse_result(line):
    path, number, name, score = line.split('\t')
    return path, int(number), name, int(score)
