"""ELF dependency closure and explicit system/graphics boundary (T308)."""
from pathlib import Path
import re
import shutil
import subprocess

# The host loader and its matching glibc must stay together. GPU implementations
# remain on the host; generic userspace loaders may be bundled as fallbacks.
SYSTEM = re.compile(r'^(?:ld-linux-x86-64\.so\.2|lib(?:c|m|dl|pthread|rt|util|resolv)\.so\.[0-9]+|libnss_[\w-]+\.so\.[0-9]+)$')
DRIVER = re.compile(r'^(?:libcuda|libnvidia|libGLX_nvidia|libEGL_nvidia)[\w.-]*\.so(?:\.[0-9]+)*$')
HOST_DIRS = '/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu:/lib64:/usr/lib64:/lib:/usr/lib'
DYNAMIC_GUI = ['libGL.so.1', 'libEGL.so.1', 'libX11.so.6', 'libXcursor.so.1', 'libXi.so.6', 'libXrandr.so.2',
               'libxcb.so.1', 'libxkbcommon.so.0', 'libxkbcommon-x11.so.0',
               'libwayland-client.so.0', 'libwayland-cursor.so.0']


def run(arguments, **kwargs):
    return subprocess.check_output(arguments, text=True, timeout=120, **kwargs)


def needed(binary):
    output = run(['readelf', '-d', str(binary)])
    names = re.findall(r'\(NEEDED\).*Shared library: \[([^\]]+)\]', output)
    for name in names:
        if not re.fullmatch(r'[A-Za-z0-9_+.-]{1,200}', name) or name in ('.', '..'):
            raise ValueError(f'invalid ELF dependency name: {name!r}')
    return names


def library_index():
    output = run(['ldconfig', '-p'])
    result = {}
    for line in output.splitlines():
        match = re.match(r'\s*(\S+) \([^)]*x86-64[^)]*\) => (/.*)$', line)
        if match:
            result.setdefault(match[1], Path(match[2]))
    return result


def verify_abi(binary):
    output = run(['objdump', '-T', str(binary)])
    versions = [tuple(map(int, v.split('.'))) for v in re.findall(r'GLIBC_([0-9.]+)', output)]
    if max(versions, default=(0,)) > (2, 36):
        raise ValueError(f'{binary}: exceeds glibc 2.36')


def closure(binaries, destination, extra=()):
    index = library_index()
    queue = list(binaries)
    for binary in queue + [index[name] for name in extra if name in index]:
        for name, path in re.findall(r'(\S+) => (/.+?) \(0x', run(['ldd', str(binary)])):
            index.setdefault(name, Path(path))
    copied = {}
    requested = set(extra)
    while queue or requested:
        if queue:
            requested.update(needed(queue.pop()))
        else:
            name = min(requested)
            requested.remove(name)
            discover(name, index, destination, copied, queue)
    return copied


def discover(name, index, destination, copied, queue):
    if name in copied or SYSTEM.fullmatch(name) or DRIVER.fullmatch(name):
        return
    source = index.get(name)
    if source is None:
        raise ValueError(f'unresolved ELF dependency: {name}')
    target = destination / name
    shutil.copy2(source.resolve(), target)
    verify_abi(target)
    copied[name] = source
    queue.append(target)


def set_app_rpath(binary, helper=False):
    # Prefer the host's matching SONAMEs for driver integration. Bundle copies
    # are fallbacks. The helper alone must prefer its sibling libevdi.
    rpath = HOST_DIRS + ':$ORIGIN/../lib'
    if helper:
        rpath = '$ORIGIN:' + rpath
    subprocess.run(['patchelf', '--force-rpath', '--set-rpath', rpath, str(binary)], check=True, timeout=30)


def stock_wrapper(program):
    if program not in ('ffmpeg', 'ffprobe', 'adb'):
        raise ValueError('unsupported stock executable')
    codec_path = '$root/usr/lib/uscreen-ffmpeg:' if program in ('ffmpeg', 'ffprobe') else ''
    return f'''#!/bin/sh
set -eu
root=$(readlink -f -- "$0")
root=${{root%/usr/bin/{program}}}
# Pin codec libraries for FFmpeg; retain host driver/loader integration.
LD_LIBRARY_PATH="{codec_path}${{LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}}{HOST_DIRS}:$root/usr/lib"
export LD_LIBRARY_PATH
exec "$root/usr/libexec/{program}" "$@"
'''


def check_loaded(binary):
    output = run(['ldd', str(binary)])
    if 'not found' in output:
        raise ValueError(f'incomplete runtime closure for {binary}: {output}')
    return output
