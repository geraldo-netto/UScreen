"""T563: the AppImage's selected codec build must survive host-library conflicts."""
from pathlib import Path
import copy
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import tarfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'packaging/appimage'))
import build
import elf
import ffmpeg_bundle as codec
import sources


class FFmpegBundleTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='uscreen ffmpeg-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def library(self, directory, identity):
        directory.mkdir(parents=True)
        source = directory / 'fixture.c'
        source.write_text(f'int uscreen_codec_identity(void) {{ return {identity}; }}\n')
        subprocess.run(['cc', '-shared', '-fPIC', '-Wl,-soname,libx264.so.164',
                        str(source), '-o', str(directory / 'libx264.so.164')], check=True)

    def test_t563_bundled_codecs_win_over_conflicting_host_library(self):
        app = self.root / 'AppDir'
        private = app / 'usr/lib/uscreen-ffmpeg'
        hostile = self.root / 'host libraries'
        self.library(private, 31)
        self.library(hostile, 81)
        source = self.root / 'main.c'
        source.write_text('#include <stdio.h>\nint uscreen_codec_identity(void);\n'
                          'int main(void) { printf("%d\\n", uscreen_codec_identity()); return 0; }\n')
        (app / 'usr/libexec').mkdir(parents=True)
        (app / 'usr/bin').mkdir()
        binary = app / 'usr/libexec/ffmpeg'
        subprocess.run(['cc', str(source), '-L' + str(private), '-l:libx264.so.164',
                        '-o', str(binary)], check=True)
        shutil.copy2(binary, app / 'usr/libexec/ffprobe')
        for name in ['ffmpeg', 'ffprobe']:
            wrapper = app / 'usr/bin' / name
            wrapper.write_text(elf.stock_wrapper(name))
            wrapper.chmod(0o755)
            result = subprocess.check_output([wrapper], text=True,
                      env=dict(os.environ, LD_LIBRARY_PATH=str(hostile)))
            self.assertEqual(result.strip(), '31', 'T563: host codec library overrode the packaged build')

    def test_t563_package_tools_come_from_pinned_prefix(self):
        prefix = self.root / 'verified prefix'
        with patch.object(build.shutil, 'which', side_effect=lambda name: '/host/bin/' + name):
            self.assertEqual(build.stock_paths(prefix), [prefix / 'bin/ffmpeg', prefix / 'bin/ffprobe',
                                                        Path('/host/bin/adb')])

    def test_t563_recipe_and_bounded_malformed_inputs(self):
        config = codec.configuration()
        self.assertEqual(config['version'], '6.1.6')
        self.assertIn('--cpu=x86-64', codec.FLAGS)
        self.assertNotIn('--enable-nonfree', codec.FLAGS)
        bad = dict(config, version='7.0')
        with patch.object(codec.Path, 'read_text', return_value=json.dumps(bad)):
            with self.assertRaises(ValueError):
                codec.configuration()
        entry = config['inputs']['ffmpeg']
        for field, values in [('sha256', ['', 'a' * 63, 'Z' * 64]),
                              ('root', ['../escape', '/absolute', '.', 'x' * 182]),
                              ('archive', ['../archive', 'bad\nname']),
                              ('url', ['http://ffmpeg.org/archive', 'https://unrelated.invalid/archive'])]:
            for value in values:
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    codec.validate_input(dict(entry, **{field: value}))
        for jobs in [-1, 0, 129, 2**32]:
            with self.assertRaises(ValueError):
                codec.prepare(self.root, jobs)

    def archive(self, path, members):
        with tarfile.open(path, 'w') as target:
            for name, kind, content in members:
                member = tarfile.TarInfo(name)
                member.type = kind
                member.size = len(content)
                target.addfile(member, io.BytesIO(content))

    def test_t563_extract_rejects_links_bounds_and_traversal_before_writing(self):
        source = self.root / 'source.tar'
        self.archive(source, [('tree/file', tarfile.REGTYPE, b'content')])
        target = self.root / 'unpacked'
        self.assertEqual(codec.extract(source, 'tree', target), target / 'tree')
        self.assertEqual((target / 'tree/file').read_bytes(), b'content')
        for name, kind in [('tree/../escape', tarfile.REGTYPE), ('/tree/file', tarfile.REGTYPE),
                           ('other/file', tarfile.REGTYPE), ('tree/link', tarfile.SYMTYPE),
                           ('tree/link', tarfile.LNKTYPE), ('tree/device', tarfile.CHRTYPE)]:
            self.archive(source, [(name, kind, b'')])
            with self.subTest(name=name, kind=kind), self.assertRaises(ValueError):
                codec.extract(source, 'tree', self.root / 'rejected')
            self.assertFalse((self.root / 'rejected').exists())
        huge = tarfile.TarInfo('tree/huge'); huge.size = 512 * 1024 * 1024 + 1
        with patch.object(codec.tarfile, 'open') as opened:
            opened.return_value.__enter__.return_value.getmembers.return_value = [huge]
            with self.assertRaises(ValueError):
                codec.extract(source, 'tree', self.root / 'rejected')

    def test_t563_cached_sources_are_rehashed_and_bad_downloads_rejected(self):
        config = codec.configuration()
        cache = self.root / 'archives'
        cache.mkdir()
        entry = config['inputs']['ffmpeg']
        path = cache / entry['archive']; path.write_bytes(b'wrong cached archive')
        with patch.object(codec.tools, 'fetch', side_effect=ValueError('checksum mismatch')):
            with self.assertRaises(ValueError):
                codec.archives(config, cache)
        expected = {name: cache / row['archive'] for name, row in config['inputs'].items()}
        for file in expected.values():
            file.write_text('verified fixture')
        with patch.object(codec.tools, 'digest', side_effect=lambda p: next(
                row['sha256'] for row in config['inputs'].values() if row['archive'] == p.name)), \
                patch.object(codec.tools, 'fetch') as fetch:
            self.assertEqual(codec.archives(config, cache), expected)
            fetch.assert_not_called()

    def test_t563_compilation_uses_pinned_headers_and_configurable_job_bound(self):
        trees = dict(ffmpeg=self.root/'ffmpeg', nvcodec=self.root/'headers-source')
        with patch.object(codec, 'command') as run:
            result = codec.compile_sources(trees, self.root, 2)
        self.assertEqual(result, self.root/'install')
        calls = run.call_args_list
        self.assertIn('PREFIX=' + str(self.root/'headers'), calls[0].args[0])
        self.assertEqual(calls[1].args[0][0], trees['ffmpeg']/'configure')
        self.assertEqual(calls[1].kwargs['env']['PKG_CONFIG_PATH'], str(self.root/'headers/lib/pkgconfig'))
        self.assertEqual(calls[2].args[0], ['make', '-j2'])
        codec.command([sys.executable, '-c', 'pass'])
        with self.assertRaises(subprocess.CalledProcessError):
            codec.command([sys.executable, '-c', 'raise SystemExit(2)'])

    def prefix(self):
        prefix = self.root / 'install'
        (prefix/'bin').mkdir(parents=True)
        for name in ['ffmpeg', 'ffprobe']:
            binary = prefix/'bin'/name
            encoders = '\n'.join(' V..... ' + encoder for encoder in codec.ENCODERS)
            binary.write_text(f'#!/bin/sh\ncase "$*" in\n*-encoders*) cat <<EOF\n{encoders}\nEOF\n;;\n'
                              f'*) printf "{name} version 6.1.6 fixture\\n";;\nesac\n')
            binary.chmod(0o755)
        manifest = dict(configuration=codec.configuration(), flags=codec.FLAGS,
                        binaries={name: codec.tools.digest(prefix/'bin'/name) for name in ['ffmpeg', 'ffprobe']})
        (prefix/'build-manifest.json').write_text(json.dumps(manifest))
        return prefix, manifest

    def test_t563_reuse_validates_manifest_binary_version_and_codec_inventory(self):
        prefix, manifest = self.prefix()
        self.assertEqual(codec.verify(prefix, codec.configuration()), manifest)
        for key, value in [('configuration', {}), ('flags', [])]:
            (prefix/'build-manifest.json').write_text(json.dumps(dict(manifest, **{key: value})))
            with self.assertRaises(ValueError):
                codec.verify(prefix, codec.configuration())
        (prefix/'build-manifest.json').write_text(json.dumps(manifest))
        binary = prefix/'bin/ffmpeg'; binary.write_text(binary.read_text().replace('6.1.6', '5.1.9'))
        with self.assertRaises(ValueError):
            codec.verify(prefix, codec.configuration())
        manifest['binaries']['ffmpeg'] = codec.tools.digest(binary)
        (prefix/'build-manifest.json').write_text(json.dumps(manifest))
        with self.assertRaises(ValueError):
            codec.verify(prefix, codec.configuration())
        with patch.object(codec.subprocess, 'check_output', return_value=' V..... libx264\n'):
            with self.assertRaises(ValueError):
                codec.verify_encoders(binary)

    def test_t563_isolation_keeps_driver_libraries_out_of_codec_directory(self):
        app = self.root/'app'; library = app/'usr/lib'; library.mkdir(parents=True)
        names = ['libx264.so.164', 'libx265.so.199', 'libvpx.so.7', 'libaom.so.3', 'libva.so.2', 'libdrm.so.2']
        for name in names:
            (library/name).write_text(name)
        codec.isolate_codecs(app, dict.fromkeys(names))
        for name in names[:4]:
            self.assertEqual((library/'uscreen-ffmpeg'/name).read_text(), name)
            self.assertFalse((library/name).exists())
        for name in names[4:]:
            self.assertTrue((library/name).is_file())

    def test_t563_prepare_retains_verified_sources_notices_and_rejects_tampering(self):
        prefix, _ = self.prefix()
        config = copy.deepcopy(codec.configuration())
        cache = self.root/'cache'; (cache/'archives').mkdir(parents=True)
        for name, entry in config['inputs'].items():
            contents = [('COPYING.GPLv2', b'GPL fixture'), ('LICENSE.md', b'FFmpeg notice')]
            if name == 'nvcodec':
                contents = [('include/ffnvcodec/nvEncodeAPI.h', b'MIT header fixture')]
            archive = cache/'archives'/entry['archive']
            self.archive(archive, [(entry['root']+'/'+path, tarfile.REGTYPE, content) for path, content in contents])
            entry['sha256'] = codec.tools.digest(archive)
        with patch.object(codec, 'configuration', return_value=config), \
                patch.object(codec, 'compile_sources', return_value=prefix) as compile:
            self.assertEqual(codec.prepare(cache, 1), prefix)
            self.assertEqual(compile.call_args.args[2], 1)
            destination = self.root/'sources'; destination.mkdir()
            notices = self.root/'notices'; notices.mkdir()
            manifest = codec.corresponding_sources(prefix, destination, notices)
            self.assertEqual(manifest['configuration'], config)
            self.assertEqual((notices/'ffmpeg/FFmpeg-LICENSE.md').read_text(), 'FFmpeg notice')
            self.assertEqual((notices/'ffmpeg/nvEncodeAPI.h').read_text(), 'MIT header fixture')
            for entry in config['inputs'].values():
                self.assertEqual(codec.tools.digest(destination/'ffmpeg'/entry['archive']), entry['sha256'])
            entry = config['inputs']['ffmpeg']
            (prefix/'sources'/entry['archive']).write_text('corrupt')
            rejected = self.root/'rejected'; rejected.mkdir()
            with self.assertRaises(ValueError):
                codec.corresponding_sources(prefix, rejected, notices)

    def test_t563_builder_prepares_codec_unless_verified_prefix_is_supplied(self):
        from types import SimpleNamespace
        args = SimpleNamespace(bundle=self.root/'bundle', output=self.root/'output', version='1.2.3',
                               evdi_source=self.root/'evdi', source_cache=self.root/'cache', tool_cache=self.root/'tools',
                               ffmpeg_prefix=None, ffmpeg_cache=self.root/'ffmpeg-cache', ffmpeg_jobs=3)
        prefix = self.root/'verified'
        def collect(_repo, _paths, destination, _notices, _evdi, _cache, supplied):
            self.assertEqual(supplied, prefix)
            destination.mkdir()
        with patch.object(codec, 'prepare', return_value=prefix) as prepare, \
                patch.object(build, 'stage', return_value=[]) as stage, \
                patch.object(sources, 'collect', side_effect=collect), \
                patch.object(build.tools, 'prepare', return_value={'appimagetool': Path('/bin/true'),
                                                                  'runtime-x86_64': self.root/'runtime'}):
            build.build(args)
            prepare.assert_called_once_with(args.ffmpeg_cache.resolve(), 3)
            self.assertEqual(stage.call_args.args[-1], prefix)


if __name__ == '__main__':
    unittest.main()
