#!/usr/bin/env python3
"""T419: isolated rectangle/H.264 replay APK; never changes production sources."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]


def verified_source_tree(sources, name, digest):
    archive = sources / (name + '-source.tar.gz')
    if hashlib.sha256(archive.read_bytes()).hexdigest() != digest:
        raise ValueError('upstream archive hash mismatch: ' + name)
    with tarfile.open(archive) as bundle:
        files = library_files(bundle)
        actual = {path.relative_to(sources / name) for path in (sources / name / 'lib').rglob('*') if path.is_file()}
        if actual != set(files):
            raise ValueError('extracted source-file set differs from pinned archive: ' + name)
        for relative, member in files.items():
            if bundle.extractfile(member).read() != (sources / name / relative).read_bytes():
                raise ValueError('extracted sources differ from pinned archive: ' + str(relative))


def library_files(bundle):
    files = {}
    for member in bundle.getmembers():
        if not member.isfile():
            continue
        relative = Path(*Path(member.name).parts[1:])
        if not relative.parts or relative.parts[0] != 'lib':
            continue
        if '..' in relative.parts or relative.is_absolute():
            raise ValueError('invalid upstream source path')
        files[relative] = member
    return files


def decoder_project():
    path = Path(__file__).with_name('decoder-project.py')
    spec = importlib.util.spec_from_file_location('project', path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def native_build(args, directory):
    compiler = args.ndk / 'toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android27-clang'
    sources = args.native_sources
    expected = {'lz4-source.tar.gz': '0b0e3aa07c8c063ddf40b082bdf7e37a1562bda40a0ff5272957f3e987e0e54b',
                'zstd-source.tar.gz': '98e9c3d949d1b924e28e01eccb7deed865eefebf25c2f21c702e5cd5b63b85e1'}
    for name, digest in expected.items():
        verified_source_tree(sources, name.removesuffix('-source.tar.gz'), digest)
    target = directory / 'app/src/main/jniLibs/arm64-v8a/librect_decode.so'
    target.parent.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).with_name('android-rect') / 'native/rect_decode.c'
    command = [str(compiler), '-O3', '-fPIC', '-shared', '-DZSTD_DISABLE_ASM',
               '-I' + str(sources / 'lz4/lib'), '-I' + str(sources / 'zstd/lib'),
               '-I' + str(sources / 'zstd/lib/common'), str(source), str(source.with_name('rect_hash.c')),
               str(sources / 'lz4/lib/lz4.c')]
    command += [str(path) for part in ['common', 'decompress'] for path in sorted((sources / 'zstd/lib' / part).glob('*.c'))]
    command += ['-Wl,-z,max-page-size=16384', '-lEGL', '-o', str(target)]
    subprocess.run(command, check=True)
    return dict(command=command, compiler=subprocess.check_output([str(compiler), '--version'], text=True),
                upstream_archives=expected, library_sha256=hashlib.sha256(target.read_bytes()).hexdigest())


def prepare(args):
    project = decoder_project()
    directory = args.directory.resolve()
    if not directory.exists():
        project.prepare(SimpleNamespace(directory=directory, package='com.uscreen.rectbench', revision=None))
        shutil.copyfile(ROOT / 'android/local.properties', directory / 'local.properties')
    for source in Path(__file__).with_name('android-rect').glob('*.kt'):
        shutil.copyfile(source, directory / 'app/src/main/java' / source.name)
    tests = directory / 'app/src/test/java'
    tests.mkdir(parents=True, exist_ok=True)
    for source in Path(__file__).with_name('android-rect-tests').glob('*.kt'):
        shutil.copyfile(source, tests / source.name)
    build = directory / 'app/build.gradle.kts'
    build.write_text(project.BUILD.replace('PACKAGE', 'com.uscreen.rectbench') + '\ndependencies { testImplementation("junit:junit:4.13.2") }\n')
    main = directory / 'app/src/main/java/MainActivity.kt'
    original = (Path(__file__).with_name('android-decoder') / 'MainActivity.kt').read_text()
    needle = 'else replay(holder)'
    assert original.count(needle) == 1
    mode = '''else if (intent.getBooleanExtra("rect", false)) RectReplay(holder.surface, active).run(
                File(filesDir, "rect.bin"), intent.getIntExtra("seconds", 8),
                intent.getIntExtra("warmup", 2), intent.getBooleanExtra("verify", false),
                intent.getBooleanExtra("mapped", false))
            else replay(holder)'''
    main.write_text(original.replace(needle, mode))
    return directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', required=True, type=Path)
    parser.add_argument('--ndk', required=True, type=Path)
    parser.add_argument('--native-sources', required=True, type=Path)
    args = parser.parse_args()
    directory = prepare(args)
    native = native_build(args, directory)
    subprocess.run([str(ROOT / 'android/gradlew'), '-p', str(directory), ':app:testDebugUnitTest', ':app:assembleDebug'], check=True)
    apk = directory / 'app/build/outputs/apk/debug/app-debug.apk'
    sources = {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
               for path in directory.rglob('*.kt') if 'build' not in path.relative_to(directory).parts}
    result = dict(package='com.uscreen.rectbench', apk_sha256=hashlib.sha256(apk.read_bytes()).hexdigest(),
                  native=native, sources=sources,
                  revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
                  source_state='working tree; exact copied Kotlin and C hashes required')
    result['native_source_sha256'] = hashlib.sha256((Path(__file__).with_name('android-rect') / 'native/rect_decode.c').read_bytes()).hexdigest()
    result['native_helper_sha256'] = hashlib.sha256((Path(__file__).with_name('android-rect') / 'native/rect_hash.c').read_bytes()).hexdigest()
    (directory / 'provenance.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
