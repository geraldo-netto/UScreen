"""T563: build a pinned, unmodified FFmpeg CLI for the Linux AppImage adapter.

Codec-internal libav libraries are linked statically, system driver libraries
remain dynamic, and external codec libraries get a private runtime directory.
This does not alter shared runtime codec selection or the in-process backend.
"""
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tempfile

import tools

FLAGS = ['--disable-shared', '--enable-static', '--enable-pic', '--disable-autodetect',
         '--disable-doc', '--disable-debug', '--disable-ffplay', '--enable-gpl',
         '--enable-libx264', '--enable-libx265', '--enable-libvpx', '--enable-libaom', '--enable-libdrm',
         '--enable-vaapi', '--enable-ffnvcodec', '--enable-nvenc', '--enable-nvdec',
         '--enable-cuvid', '--enable-pthreads', '--enable-zlib', '--cpu=x86-64']
CODEC_LIBRARY = re.compile(r'^lib(?:x264|x265|vpx|aom)\.so(?:\.[0-9]+)+$')
ENCODERS = ['libx264', 'libx265', 'libvpx-vp9', 'libaom-av1', 'h264_vaapi', 'hevc_vaapi',
            'vp9_vaapi', 'av1_vaapi', 'h264_nvenc', 'hevc_nvenc', 'av1_nvenc']


def configuration():
    value = json.loads(Path(__file__).with_name('ffmpeg.json').read_text())
    if not re.fullmatch(r'6\.[0-9]+\.[0-9]+', value['version']):
        raise ValueError('expected pinned FFmpeg 6.x release')
    for entry in value['inputs'].values():
        validate_input(entry)
    return value


def validate_input(entry):
    if not re.fullmatch(r'[a-f0-9]{64}', entry['sha256']):
        raise ValueError('invalid source digest')
    for name in ('archive', 'root'):
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{1,180}', entry[name]):
            raise ValueError('invalid source name')
    if not entry['url'].startswith(('https://ffmpeg.org/releases/',
                                   'https://codeload.github.com/FFmpeg/nv-codec-headers/tar.gz/')):
        raise ValueError('unexpected source origin')


def archives(config, cache):
    cache.mkdir(parents=True, exist_ok=True)
    result = {}
    for name, entry in config['inputs'].items():
        target = cache / entry['archive']
        if not target.is_file() or tools.digest(target) != entry['sha256']:
            tools.fetch(entry, target)
        result[name] = target
    return result


def extract(source, root, destination):
    with tarfile.open(source) as archive:
        members = archive.getmembers()
        if sum(member.size for member in members) > 512 * 1024 * 1024:
            raise ValueError('source tree exceeds extraction limit')
        for member in members:
            validate_member(member, root)
        # Only verified regular files/directories; no links, devices or escapes.
        archive.extractall(destination, members=members)
    return destination / root


def validate_member(member, root):
    path = PurePosixPath(member.name)
    if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0] != root:
        raise ValueError('source archive path escapes its root')
    if not (member.isfile() or member.isdir()):
        raise ValueError('source archive contains a link or special file')


def command(arguments, **kwargs):
    subprocess.run([str(value) for value in arguments], check=True, timeout=3600, **kwargs)


def compile_sources(trees, work, jobs):
    prefix = work / 'install'
    headers = work / 'headers'
    command(['make', '-C', trees['nvcodec'], 'install', f'PREFIX={headers}'])
    build = work / 'objects'
    build.mkdir()
    environment = dict(os.environ, PKG_CONFIG_PATH=str(headers / 'lib/pkgconfig'))
    command([trees['ffmpeg'] / 'configure', '--prefix=' + str(prefix), *FLAGS], cwd=build, env=environment)
    command(['make', f'-j{jobs}'], cwd=build)
    command(['make', 'install'], cwd=build)
    return prefix


def verify(prefix, config):
    manifest = json.loads((prefix / 'build-manifest.json').read_text())
    if manifest['configuration'] != config or manifest['flags'] != FLAGS:
        raise ValueError('FFmpeg build does not match the pinned recipe')
    for name in ['ffmpeg', 'ffprobe']:
        binary = prefix / 'bin' / name
        if tools.digest(binary) != manifest['binaries'][name]:
            raise ValueError('FFmpeg binary checksum differs from build manifest')
        version = subprocess.check_output([binary, '-version'], text=True, timeout=30).splitlines()[0]
        if not version.startswith(f'{name} version {config["version"]} '):
            raise ValueError('FFmpeg executable version differs from pinned release')
    verify_encoders(prefix / 'bin/ffmpeg')
    return manifest


def verify_encoders(binary):
    text = subprocess.check_output([binary, '-hide_banner', '-encoders'], text=True, timeout=30)
    present = set(re.findall(r'^ [VAS][.A-Z]{5} (\S+)', text, re.M))
    missing = set(ENCODERS) - present
    if missing:
        raise ValueError(f'FFmpeg build lacks required encoders: {sorted(missing)}')


def retain_sources(prefix, trees, inputs, config):
    destination = prefix / 'sources'
    destination.mkdir()
    for source in inputs.values():
        shutil.copy2(source, destination / source.name)
    notices = prefix / 'notices'
    notices.mkdir()
    for path in trees['ffmpeg'].glob('COPYING*'):
        shutil.copy2(path, notices / path.name)
    shutil.copy2(trees['ffmpeg'] / 'LICENSE.md', notices / 'FFmpeg-LICENSE.md')
    # nv-codec-headers carries its MIT notice in each header, not a LICENSE file.
    shutil.copy2(trees['nvcodec'] / 'include/ffnvcodec/nvEncodeAPI.h', notices / 'nvEncodeAPI.h')
    manifest = dict(configuration=config, flags=FLAGS,
                    compiler=subprocess.check_output(['cc', '--version'], text=True),
                    binaries={name: tools.digest(prefix / 'bin' / name) for name in ['ffmpeg', 'ffprobe']})
    (prefix / 'build-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


def prepare(cache, jobs=2):
    if not 1 <= jobs <= 128:
        raise ValueError('FFmpeg build jobs must be between 1 and 128')
    config = configuration()
    inputs = archives(config, cache / 'archives')
    # Keep failed builds available for inspection; never overwrite another tree.
    work = Path(tempfile.mkdtemp(prefix='ffmpeg-build-', dir=cache)).resolve()
    trees = {name: extract(path, config['inputs'][name]['root'], work) for name, path in inputs.items()}
    prefix = compile_sources(trees, work, jobs)
    retain_sources(prefix, trees, inputs, config)
    verify(prefix, config)
    return prefix


def isolate_codecs(appdir, dependencies):
    private = appdir / 'usr/lib/uscreen-ffmpeg'
    private.mkdir()
    for name in dependencies:
        if CODEC_LIBRARY.fullmatch(name):
            (appdir / 'usr/lib' / name).replace(private / name)


def corresponding_sources(prefix, destination, notices):
    manifest = verify(prefix, configuration())
    target = destination / 'ffmpeg'
    target.mkdir()
    for entry in manifest['configuration']['inputs'].values():
        source = prefix / 'sources' / entry['archive']
        if tools.digest(source) != entry['sha256']:
            raise ValueError('FFmpeg corresponding source checksum mismatch')
        shutil.copy2(source, target / source.name)
    shutil.copytree(prefix / 'notices', notices / 'ffmpeg')
    shutil.copy2(prefix / 'build-manifest.json', target / 'build-manifest.json')
    return manifest
