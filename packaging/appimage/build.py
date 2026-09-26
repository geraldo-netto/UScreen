#!/usr/bin/env python3
"""Build the x86-64 AppImage and corresponding-source asset inside Debian 12."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

import elf
import ffmpeg_bundle
import sources
import tools


def copy(source, target, mode=None):
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    if mode is not None:
        target.chmod(mode)


def stage(repo, bundle, appdir, ffmpeg_prefix):
    ffmpeg_bundle.verify(ffmpeg_prefix, ffmpeg_bundle.configuration())
    binary = appdir / 'usr/bin'
    binary.mkdir(parents=True)
    library = appdir / 'usr/lib'
    library.mkdir()
    for name in ('blent', 'blent-gui', 'evdi_helper', 'libevdi.so.1.15.0'):
        copy(bundle / 'bin' / name, binary / name)
    (binary / 'libevdi.so.1').symlink_to('libevdi.so.1.15.0')
    programs = [binary / name for name in ('blent', 'blent-gui', 'evdi_helper', 'libevdi.so.1.15.0')]
    stock = stage_stock(appdir, ffmpeg_prefix)
    copy(Path('/bin/bash'), binary / 'bash')
    programs.extend([binary / 'bash', *stock])
    for program in programs:
        elf.verify_abi(program)
    dependencies = elf.closure(programs, library, elf.DYNAMIC_GUI)
    # libevdi is kept as a replaceable, unmodified sibling of the helper.
    (library / 'libevdi.so.1').unlink(missing_ok=True)
    dependencies.pop('libevdi.so.1', None)
    ffmpeg_bundle.isolate_codecs(appdir, dependencies)
    for name in ('blent', 'blent-gui', 'evdi_helper', 'bash'):
        elf.set_app_rpath(binary / name, helper=name == 'evdi_helper')
        elf.check_loaded(binary / name)
    stage_metadata(repo, appdir)
    # Upstream FFmpeg is accounted for separately from Debian source packages.
    return [Path('/bin/bash'), stock_paths(ffmpeg_prefix)[-1], *dependencies.values()]


def stock_paths(ffmpeg_prefix):
    return [ffmpeg_prefix / 'bin/ffmpeg', ffmpeg_prefix / 'bin/ffprobe',
            Path(shutil.which('adb') or '/nonexistent/adb')]


def stage_stock(appdir, ffmpeg_prefix):
    result = []
    for source in stock_paths(ffmpeg_prefix):
        target = appdir / 'usr/libexec' / source.name
        copy(source, target)
        wrapper = appdir / 'usr/bin' / source.name
        wrapper.write_text(elf.stock_wrapper(source.name))
        wrapper.chmod(0o755)
        result.append(target)
    return result


def stage_metadata(repo, appdir):
    source = repo / 'packaging/appimage'
    copy(source / 'AppRun', appdir / 'AppRun', 0o755)
    share = appdir / 'usr/share/blent'
    copy(source / 'install-appimage.sh', share / 'install-appimage.sh', 0o755)
    for name in ('write-desktop-entry.sh', 'write-systemd-service.sh', 'setup-evdi.sh', 'blent.service',
                 'blent-service-autostart.desktop'):
        copy(repo / 'scripts' / name, share / name)
    copy(repo / 'scripts/blent.desktop', share / 'blent.desktop')
    desktop = (repo / 'scripts/blent.desktop').read_text().replace('Exec=blent-gui', 'Exec=AppRun')
    (appdir / 'blent.desktop').write_text(desktop)
    icon = repo / 'packaging/icons/blent.svg'
    copy(icon, appdir / 'blent.svg')
    copy(icon, appdir / 'usr/share/icons/hicolor/scalable/apps/blent.svg')
    subprocess.run([str(repo / 'scripts/copy-distribution-docs.sh'), str(appdir / 'usr/share/doc/blent')],
                   cwd=repo, check=True)


def build(args):
    repo = Path(__file__).resolve().parents[2]
    bundle, output = args.bundle.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for suffix in ('x86_64.AppImage', 'AppImage-sources.tar.gz'):
        (output / f'blent-{args.version}-{suffix}').unlink(missing_ok=True)
    work = output / 'appimage-work'
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    appdir = work / 'Blent.AppDir'
    ffmpeg_prefix = args.ffmpeg_prefix or ffmpeg_bundle.prepare(args.ffmpeg_cache.resolve(), args.ffmpeg_jobs)
    paths = stage(repo, bundle, appdir, ffmpeg_prefix.resolve())
    source_dir = work / 'sources'
    sources.collect(repo, paths, source_dir, appdir / 'usr/share/doc/blent/bundled',
                    args.evdi_source, args.source_cache.resolve(), ffmpeg_prefix.resolve())
    with tarfile.open(output / f'blent-{args.version}-AppImage-sources.tar.gz', 'w:gz') as archive:
        archive.add(source_dir, arcname='sources')
    pinned = tools.prepare(args.tool_cache)
    subprocess.run([str(pinned['appimagetool']), '--appimage-extract-and-run', '--no-appstream',
                    '--runtime-file', str(pinned['runtime-x86_64']), '--mksquashfs-opt', '-processors',
                    '--mksquashfs-opt', '2', str(appdir), str(output / f'blent-{args.version}-x86_64.AppImage')],
                   check=True, env=dict(os.environ, ARCH='x86_64'))


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--evdi-source', type=Path, default=Path('/opt/evdi'))
    parser.add_argument('--source-cache', type=Path, default=Path('target-appimage-sources'))
    parser.add_argument('--tool-cache', type=Path, default=Path('target-appimage-tools'))
    parser.add_argument('--ffmpeg-cache', type=Path, default=Path('target-appimage-ffmpeg'))
    parser.add_argument('--ffmpeg-jobs', type=int, choices=range(1, 129), default=2, metavar='N')
    parser.add_argument('--ffmpeg-prefix', type=Path, help='reuse a verified pinned build, including its source manifest')
    args = parser.parse_args()
    import re
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', args.version):
        parser.error('version must contain three numeric components')
    return args


if __name__ == '__main__':
    build(arguments())
