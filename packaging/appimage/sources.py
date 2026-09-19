"""Collect exact corresponding sources and notices for bundled binaries (T308)."""
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

from tools import digest
from elf import run


def package_for(path):
    candidates = [str(path), str(path.resolve())]
    if str(path).startswith('/usr/'):
        candidates.append(str(path)[4:])
    for candidate in candidates:
        result = subprocess.run(['dpkg-query', '-S', candidate], capture_output=True, text=True, timeout=30)
        if result.returncode == 0:
            return result.stdout.split(': ')[0].splitlines()[0]
    raise ValueError(f'no source package owner for {path}')


def package_info(package):
    text = run(['dpkg-query', '-W', '-f=${source:Package}\t${source:Version}', package])
    name, version = text.split('\t')
    if not name or not version or '/' in name + version:
        raise ValueError(f'invalid source package metadata: {package}')
    return name, version


def debian_sources(paths, destination, notices, cache):
    packages = sorted({package_for(path) for path in paths})
    sources = set()
    for package in packages:
        source = package_info(package)
        sources.add(source)
        copyright_file = Path('/usr/share/doc') / package.split(':')[0] / 'copyright'
        shutil.copyfile(copyright_file, notices / (package.replace(':', '_') + '.copyright'))
    cache.mkdir(parents=True, exist_ok=True)
    for name, version in sorted(sources):
        download_debian_source(name, version, cache, destination)
    return [dict(package=package, source=package_info(package)) for package in packages]


def download_debian_source(name, version, cache, destination):
    subprocess.run(['apt-get', 'source', '--download-only', '--only-source', f'{name}={version}'],
                   cwd=cache, check=True, timeout=600)
    descriptor = cache / f"{name}_{version.split(':')[-1]}.dsc"
    shutil.copyfile(descriptor, destination / descriptor.name)
    for filename, expected in source_checksums(descriptor.read_text()):
        source = cache / filename
        if digest(source) != expected:
            raise ValueError(f'corresponding-source checksum mismatch: {filename}')
        shutil.copyfile(source, destination / filename)


def source_checksums(text):
    import re
    match = re.search(r'^Checksums-Sha256:\n((?: [^\n]+\n)+)', text, re.M)
    if match is None:
        raise ValueError('source descriptor lacks SHA256 checksums')
    files = []
    for line in match[1].splitlines():
        checksum, size, name = line.split()
        if not re.fullmatch(r'[a-f0-9]{64}', checksum) or not size.isdigit():
            raise ValueError('invalid source checksum record')
        if not re.fullmatch(r'[A-Za-z0-9_+.~-]+', name) or name in ('.', '..'):
            raise ValueError('invalid source archive name')
        files.append((name, checksum))
    return files


def rust_sources(repo, destination, notices):
    metadata = json.loads(run(['cargo', 'metadata', '--locked', '--format-version=1',
                              '--filter-platform=x86_64-unknown-linux-gnu'], cwd=repo))
    dependencies = {node['id'] for node in metadata['resolve']['nodes']}
    entries = []
    for package in metadata['packages']:
        if package['id'] in dependencies and package['source']:
            entries.append(copy_crate(package, destination, notices))
    return entries


def copy_crate(package, destination, notices):
    source = Path(package['manifest_path']).parent
    name = f"{package['name']}-{package['version']}"
    target = destination / (name + '.tar.gz')
    copy_crate_notices(source, notices / name)
    with tarfile.open(target, 'w:gz') as archive:
        archive.add(source, arcname=target.name.removesuffix('.tar.gz'))
    return dict(name=package['name'], version=package['version'], license=package['license'],
                source=package['source'], sha256=digest(target))


def copy_crate_notices(source, destination):
    destination.mkdir()
    for path in source.iterdir():
        if path.name.lower().startswith(('license', 'licence', 'copying', 'copyright', 'notice')):
            if path.is_dir():
                shutil.copytree(path, destination / path.name)
            else:
                shutil.copyfile(path, destination / path.name)


def runtime_sources(destination, notices):
    from tools import transfer
    target = destination / 'appimage-runtime-20251108.tar.gz'
    url = 'https://codeload.github.com/AppImage/type2-runtime/tar.gz/refs/tags/20251108'
    with urllib.request.urlopen(url, timeout=60) as response, target.open('wb') as output:
        transfer(response, output)
    # Reading one known regular file avoids extracting an untrusted tar tree.
    with tarfile.open(target) as archive:
        member = archive.getmember('type2-runtime-20251108/LICENSE')
        if not member.isfile() or member.size > 1024 * 1024:
            raise ValueError('invalid AppImage runtime license')
        with archive.extractfile(member) as stream:
            (notices / 'AppImage-runtime-LICENSE').write_bytes(stream.read())
    dependencies = runtime_dependencies(destination, notices)
    return dict(url=url, sha256=digest(target), dependencies=dependencies)


def runtime_dependencies(destination, notices):
    import re
    from tools import fetch
    inputs = json.loads(Path(__file__).with_name('runtime-inputs.json').read_text())
    for name, entry in inputs.items():
        if not re.fullmatch(r'[a-f0-9]{64}', entry['sha256']) or not entry['url'].startswith('https://'):
            raise ValueError('invalid pinned runtime source')
        target = destination / name
        fetch(entry, target)
        with tarfile.open(target) as archive:
            member = archive.getmember(entry['license'])
            if not member.isfile() or member.size > 1024 * 1024:
                raise ValueError('invalid runtime dependency license')
            with archive.extractfile(member) as stream:
                (notices / (name + '.LICENSE')).write_bytes(stream.read())
    return inputs


def verify_evdi(repo, source):
    import re
    release = (repo / 'scripts/build-release.sh').read_text()
    expected = re.search(r'^EVDI_COMMIT="([0-9a-f]{40})"$', release, re.M)
    actual = run(['git', '-C', str(source), 'rev-parse', 'HEAD']).strip()
    dirty = run(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no'])
    if expected is None or actual != expected[1] or dirty.strip():
        raise ValueError('AppImage requires the pinned, unmodified EVDI source')
    return actual


def collect(repo, paths, destination, notices, evdi, cache):
    revision = verify_evdi(repo, evdi)
    destination.mkdir(parents=True)
    notices.mkdir(parents=True)
    debian = destination / 'debian'
    debian.mkdir()
    crates = destination / 'rust'
    crates.mkdir()
    manifest = dict(debian=debian_sources(paths, debian, notices, cache), rust=rust_sources(repo, crates, notices))
    subprocess.run(['git', '-C', str(evdi), 'archive', '--format=tar.gz',
                    '--output=' + str(destination / 'libevdi-v1.15.0.tar.gz'), 'HEAD'], check=True)
    manifest['evdi_revision'] = revision
    manifest['runtime'] = runtime_sources(destination, notices)
    text = json.dumps(manifest, indent=2) + '\n'
    (destination / 'manifest.json').write_text(text)
    (notices / 'manifest.json').write_text(text)
    return manifest
