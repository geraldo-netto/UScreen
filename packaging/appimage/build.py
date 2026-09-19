#!/usr/bin/env python3
"""Build the x86-64 AppImage and corresponding-source asset inside Debian 12."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

import elf
import sources
import tools


def copy(source, target, mode=None):
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    if mode is not None:
        target.chmod(mode)


def stage(repo, bundle, appdir):
    binary = appdir / 'usr/bin'
    binary.mkdir(parents=True)
    library = appdir / 'usr/lib'
    library.mkdir()
    for name in ('uscreen', 'uscreen-gui', 'evdi_helper', 'libevdi.so.1.15.0'):
        copy(bundle / 'bin' / name, binary / name)
    (binary / 'libevdi.so.1').symlink_to('libevdi.so.1.15.0')
    programs = [binary / name for name in ('uscreen', 'uscreen-gui', 'evdi_helper', 'libevdi.so.1.15.0')]
    stock = stage_stock(appdir)
    copy(Path('/bin/bash'), binary / 'bash')
    programs.extend([binary / 'bash', *stock])
    for program in programs:
        elf.verify_abi(program)
    dependencies = elf.closure(programs, library, elf.DYNAMIC_GUI)
    # libevdi is kept as a replaceable, unmodified sibling of the helper.
    (library / 'libevdi.so.1').unlink(missing_ok=True)
    dependencies.pop('libevdi.so.1', None)
    for name in ('uscreen', 'uscreen-gui', 'evdi_helper', 'bash'):
        elf.set_app_rpath(binary / name, helper=name == 'evdi_helper')
        elf.check_loaded(binary / name)
    stage_metadata(repo, appdir)
    return [Path('/bin/bash'), *stock_paths(), *dependencies.values()]


def stock_paths():
    return [Path(shutil.which(program) or '/nonexistent/' + program) for program in ('ffmpeg', 'ffprobe', 'adb')]


def stage_stock(appdir):
    result = []
    for source in stock_paths():
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
    share = appdir / 'usr/share/uscreen'
    copy(source / 'install-appimage.sh', share / 'install-appimage.sh', 0o755)
    for name in ('write-desktop-entry.sh', 'write-systemd-service.sh', 'setup-evdi.sh', 'uscreen.service'):
        copy(repo / 'scripts' / name, share / name)
    copy(repo / 'scripts/uscreen.desktop', share / 'uscreen.desktop')
    desktop = (repo / 'scripts/uscreen.desktop').read_text().replace('Exec=uscreen-gui', 'Exec=AppRun')
    (appdir / 'uscreen.desktop').write_text(desktop)
    icon = repo / 'packaging/icons/uscreen.svg'
    copy(icon, appdir / 'uscreen.svg')
    copy(icon, appdir / 'usr/share/icons/hicolor/scalable/apps/uscreen.svg')
    subprocess.run([str(repo / 'scripts/copy-distribution-docs.sh'), str(appdir / 'usr/share/doc/uscreen')],
                   cwd=repo, check=True)


def build(args):
    repo = Path(__file__).resolve().parents[2]
    bundle, output = args.bundle.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for suffix in ('x86_64.AppImage', 'AppImage-sources.tar.gz'):
        (output / f'uscreen-{args.version}-{suffix}').unlink(missing_ok=True)
    work = output / 'appimage-work'
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    appdir = work / 'UScreen.AppDir'
    paths = stage(repo, bundle, appdir)
    source_dir = work / 'sources'
    sources.collect(repo, paths, source_dir, appdir / 'usr/share/doc/uscreen/bundled', args.evdi_source, args.source_cache.resolve())
    with tarfile.open(output / f'uscreen-{args.version}-AppImage-sources.tar.gz', 'w:gz') as archive:
        archive.add(source_dir, arcname='sources')
    pinned = tools.prepare(args.tool_cache)
    subprocess.run([str(pinned['appimagetool']), '--appimage-extract-and-run', '--no-appstream',
                    '--runtime-file', str(pinned['runtime-x86_64']), '--mksquashfs-opt', '-processors',
                    '--mksquashfs-opt', '2', str(appdir), str(output / f'uscreen-{args.version}-x86_64.AppImage')],
                   check=True, env=dict(os.environ, ARCH='x86_64'))


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--evdi-source', type=Path, default=Path('/opt/evdi'))
    parser.add_argument('--source-cache', type=Path, default=Path('target-appimage-sources'))
    parser.add_argument('--tool-cache', type=Path, default=Path('target-appimage-tools'))
    args = parser.parse_args()
    import re
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', args.version):
        parser.error('version must contain three numeric components')
    return args


if __name__ == '__main__':
    build(arguments())
