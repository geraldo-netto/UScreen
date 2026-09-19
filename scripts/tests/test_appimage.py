"""T308: portable launch, service paths, closure and malformed packaging inputs."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / 'packaging/appimage'))
import elf
import tools
import sources
import build


class AppImageTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='uscreen-image space-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.app = self.root / 'source.AppDir'
        (self.app / 'usr/bin').mkdir(parents=True)
        self.write(self.app / 'AppRun', (REPO / 'packaging/appimage/AppRun').read_text())
        self.env = dict(os.environ, HOME=str(self.root / 'home'), APPDIR=str(self.app),
                        XDG_DATA_HOME=str(self.root / 'data % $'), XDG_CONFIG_HOME=str(self.root / 'config'),
                        USCREEN_TEST_OUTPUT=str(self.root / 'output'))
        self.env.pop('APPIMAGE', None)
        self.env.pop('USCREEN_APPIMAGE_LAUNCHER', None)
        self.env.pop('LD_LIBRARY_PATH', None)

    def write(self, path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(0o755)
        return path

    def invoke(self, *args, env=None):
        return subprocess.run([str(self.app / 'AppRun'), *args], env=env or self.env,
                              capture_output=True, text=True, timeout=20)

    def test_t308_dispatch_uses_stable_outer_path_without_global_loader_overrides(self):
        script = '#!/bin/sh\nprintf "%s\\n" "$0" "$USCREEN_APPIMAGE_LAUNCHER" "$APPIMAGE_EXTRACT_AND_RUN" "${LD_LIBRARY_PATH-unset}" "$@"\n'
        for name in ('uscreen', 'uscreen-gui', 'bash'):
            self.write(self.app / 'usr/bin' / name, script)
        outer = self.write(self.root / 'outer %h.AppImage', '#!/bin/sh\nexit 0\n')
        for arguments, program in [((), 'uscreen-gui'), (('--gui', 'arg'), 'uscreen-gui'),
                                   (('--daemon', 'status'), 'uscreen'), (('status',), 'uscreen'),
                                   (('--install-user',), 'bash')]:
            with self.subTest(arguments=arguments):
                result = self.invoke(*arguments, env=dict(self.env, APPIMAGE=str(outer)))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines()[:4],
                                 [str(self.app / 'usr/bin' / program), str(outer), '1', 'unset'])
        self.assertIn(str(self.app / 'AppRun'), self.invoke('status').stdout)

    def installer_fixture(self):
        build.stage_metadata(REPO, self.app)
        (self.app / 'usr/bin/bash').symlink_to('/bin/bash')
        self.write(self.app / 'usr/bin/uscreen', '#!/bin/sh\nprintf "%s\\n" "${USCREEN_TEST_STATE-uscreen is not running}"\n')
        self.write(self.app / 'usr/bin/systemctl', '#!/bin/sh\nexit 0\n')

    def test_t308_user_registration_has_stable_quoted_paths_and_preserves_settings(self):
        self.installer_fixture()
        result = self.invoke('--install-user')
        self.assertEqual(result.returncode, 0, result.stderr)
        launcher = Path(self.env['XDG_DATA_HOME']) / 'uscreen/appimage/UScreen.AppDir/AppRun'
        unit = (Path(self.env['XDG_CONFIG_HOME']) / 'systemd/user/uscreen.service').read_text()
        self.assertIn('# USCREEN_APPIMAGE_PATH_HEX=' + str(launcher).encode().hex(), unit)
        self.assertNotIn(str(self.app), unit)
        self.assertIn(str(launcher).replace('%', '%%'), unit)
        desktop = (Path(self.env['XDG_DATA_HOME']) / 'applications/uscreen.desktop').read_text()
        self.assertIn('"--gui"', desktop)
        self.assertTrue(launcher.is_file())
        link = Path(self.env['HOME']) / '.local/bin/uscreen'
        self.assertTrue(link.is_symlink())
        link.unlink()
        link.mkdir()
        self.assertNotEqual(self.invoke('--install-user').returncode, 0)
        self.assertEqual(list(link.iterdir()), [])
        link.rmdir()
        self.assertEqual(self.invoke('--install-user').returncode, 0)

    def test_t308_registration_rejects_running_daemon_and_unknown_options(self):
        self.installer_fixture()
        running = self.invoke('--install-user', env=dict(self.env, USCREEN_TEST_STATE='uscreen is running (PID: 123)'))
        self.assertNotEqual(running.returncode, 0)
        self.assertFalse(Path(self.env['XDG_DATA_HOME']).exists())
        self.assertNotEqual(self.invoke('--install-user', '--unknown').returncode, 0)
        for home in ('', 'relative'):
            self.assertNotEqual(self.invoke('--install-user', env=dict(self.env, HOME=home)).returncode, 0)

    def test_t308_image_copy_survives_source_deletion_and_keeps_preferences(self):
        self.installer_fixture()
        outer = self.write(self.root / 'download.AppImage', '#!/bin/sh\nprintf "stable image\\n"\n')
        env = dict(self.env, APPIMAGE=str(outer), XDG_DATA_HOME='relative-is-ignored')
        result = self.invoke('--install-user', env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        outer.unlink()
        installed = Path(self.env['HOME']) / '.local/share/uscreen/appimage/UScreen.AppImage'
        self.assertEqual(subprocess.check_output([installed], text=True), 'stable image\n')

    def test_t308_stock_wrappers_reject_unknown_programs_and_preserve_arguments(self):
        for name in ('ffmpeg', 'ffprobe', 'adb'):
            target = self.app / 'usr/libexec' / name
            self.write(target, '#!/bin/sh\nprintf "%s\\n" "$@" "$LD_LIBRARY_PATH"\n')
            wrapper = self.write(self.app / 'usr/bin' / name, elf.stock_wrapper(name))
            result = subprocess.run([wrapper, 'space value', '$HOME'], env=self.env, capture_output=True, text=True)
            self.assertEqual(result.stdout.splitlines()[:2], ['space value', '$HOME'])
            self.assertIn(str(self.app / 'usr/lib'), result.stdout)
        for value in ('', '../ffmpeg', 'ffmpeg\n', 'a' * 4096):
            with self.assertRaises(ValueError):
                elf.stock_wrapper(value)

    def test_t308_invalid_elf_names_and_abi_fail_closed(self):
        for name in ('../bad', '/', '..', 'a' * 201, 'foo\nbar'):
            with patch.object(elf, 'run', return_value=f'(NEEDED) Shared library: [{name}]'):
                with self.assertRaises(ValueError):
                    elf.needed('fixture')
        with patch.object(elf, 'run', return_value='(NEEDED) Shared library: [libc.so.6]'):
            self.assertEqual(elf.needed('fixture'), ['libc.so.6'])
        for version in ('2.36', '2.35', '2.2.5'):
            with patch.object(elf, 'run', return_value='GLIBC_' + version):
                elf.verify_abi('fixture')
        with patch.object(elf, 'run', return_value='GLIBC_2.37'):
            with self.assertRaises(ValueError):
                elf.verify_abi('fixture')
        with self.assertRaises(ValueError):
            elf.discover('missing.so', {}, self.root, {}, [])

    def test_t308_dependency_closure_terminates_cycles_and_excludes_host_glibc(self):
        source = self.write(self.root / 'libfixture.so', 'ELF fixture')
        destination = self.root / 'lib'
        destination.mkdir()
        with patch.object(elf, 'library_index', return_value={'libfixture.so': source}), \
             patch.object(elf, 'run', return_value=''), patch.object(elf, 'verify_abi'), \
             patch.object(elf, 'needed', return_value=['libfixture.so', 'libc.so.6', 'libnvidia-glcore.so.1']):
            found = elf.closure([source], destination)
        self.assertEqual(found, {'libfixture.so': source})
        self.assertEqual(list(destination.iterdir()), [destination / 'libfixture.so'])

    def test_t308_pinned_tool_validation_download_and_bounds(self):
        data = b'verified fixture'
        entry = dict(url='https://github.com/AppImage/fixture', sha256=hashlib.sha256(data).hexdigest())
        target = self.root / 'tool'
        with patch.object(tools.urllib.request, 'urlopen', return_value=io.BytesIO(data)):
            tools.download(entry, target)
        self.assertEqual(target.read_bytes(), data)
        with patch.object(tools.urllib.request, 'urlopen', return_value=io.BytesIO(b'wrong')):
            with self.assertRaises(ValueError):
                tools.download(entry, target)
        self.assertEqual(target.read_bytes(), data)
        for invalid in ({}, dict(entry, sha256='0' * 63), dict(entry, url='http://example.invalid/tool')):
            with self.assertRaises(ValueError):
                tools.validate(invalid)
        class Oversized:
            def read(self, _):
                return b'x' * (64 * 1024 * 1024 + 1)
        with self.assertRaises(ValueError):
            tools.transfer(Oversized(), io.BytesIO())

    def test_t308_daemon_extraction_survives_gui_exit(self):
        outer = self.write(self.root / 'outer image', (REPO / 'scripts/tests/appimage_lifetime_fixture.py').read_text())
        daemon = """#!/usr/bin/env python3
import os,time
from pathlib import Path
root=Path(os.environ['USCREEN_TEST_OUTPUT'])
root.write_text(os.environ['APPDIR'])
while not root.with_suffix('.stop').exists(): time.sleep(0.01)
root.with_suffix('.alive').write_text(str(Path(os.environ['APPDIR']).is_dir()))
"""
        gui = """#!/usr/bin/env python3
import os,subprocess,time
from pathlib import Path
subprocess.Popen([os.environ['USCREEN_APPIMAGE_LAUNCHER'], 'start'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
root=Path(os.environ['USCREEN_TEST_OUTPUT'])
for _ in range(1000):
    if root.exists(): break
    time.sleep(0.01)
else: raise SystemExit('daemon did not start')
root.with_suffix('.gui').write_text(os.environ['APPDIR'])
"""
        self.write(self.app / 'usr/bin/uscreen', daemon)
        self.write(self.app / 'usr/bin/uscreen-gui', gui)
        env = dict(self.env, USCREEN_TEST_TEMPLATE=str(self.app))
        result = subprocess.run([outer], env=env, capture_output=True, text=True, timeout=20)
        output = Path(self.env['USCREEN_TEST_OUTPUT'])
        try:
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(Path(output.with_suffix('.gui').read_text()).exists())
            self.assertTrue(Path(output.read_text()).exists())
        finally:
            output.with_suffix('.stop').touch()
        import time
        for _ in range(1000):
            if output.with_suffix('.alive').exists(): break
            time.sleep(0.01)
        self.assertEqual(output.with_suffix('.alive').read_text(), 'True')

    def test_t308_autostart_redirect_and_nested_destination(self):
        self.installer_fixture()
        self.write(self.app / 'usr/bin/systemctl', '#!/bin/sh\nexit 1\n')
        entry = Path(self.env['XDG_CONFIG_HOME']) / 'autostart/uscreen.desktop'
        self.write(entry, '[Desktop Entry]\nType=Application\nExec=/old/uscreen start\nHidden=true\n')
        result = self.invoke('--install-user')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Hidden=true', entry.read_text())
        self.assertNotIn('/old/uscreen', entry.read_text())
        self.write(self.app / 'usr/bin/systemctl', '#!/bin/sh\nexit 0\n')
        self.assertEqual(self.invoke('--install-user').returncode, 0)
        self.assertFalse(entry.exists())
        result = self.invoke('--install-user', env=dict(self.env, XDG_DATA_HOME=str(self.app / 'nested')))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('outside', result.stderr)

    def test_t308_loader_index_and_commands(self):
        with patch.object(elf, 'run', return_value=' libX.so.1 (libc6,x86-64) => /lib/libX.so.1\ninvalid\n'):
            self.assertEqual(elf.library_index(), {'libX.so.1': Path('/lib/libX.so.1')})
        with patch.object(elf.subprocess, 'run') as command:
            elf.set_app_rpath('binary')
            elf.set_app_rpath('helper', helper=True)
            self.assertIn('$ORIGIN:', command.call_args.args[0][3])
        with patch.object(elf, 'run', return_value='resolved'):
            self.assertEqual(elf.check_loaded('fixture'), 'resolved')
        with patch.object(elf, 'run', return_value='libfoo => not found'):
            with self.assertRaises(ValueError): elf.check_loaded('fixture')
        self.assertEqual(elf.run(['/bin/echo', 'fixture']).strip(), 'fixture')

    def test_t308_pinned_cache_and_malformed_metadata_fuzz(self):
        entry = dict(url='https://github.com/AppImage/fixture', sha256=hashlib.sha256(b'fixture').hexdigest())
        (self.root / 'tools.json').write_text(json.dumps({'tool': entry}))
        target = self.write(self.root / 'tool', 'fixture')
        with patch.object(tools, '__file__', str(self.root / 'tools.py')):
            self.assertEqual(tools.prepare(self.root), {'tool': target})
            target.write_text('corrupt')
            with patch.object(tools.urllib.request, 'urlopen', return_value=io.BytesIO(b'fixture')):
                tools.prepare(self.root)
        import random
        rng = random.Random(308)
        for _ in range(256):
            value = rng.choice([None, False, [], 42, {'sha256': rng.randbytes(16).hex()}, {'sha256': '0'*64, 'url': None}])
            with self.assertRaises(ValueError): tools.validate(value)

    def test_t308_source_descriptor_corruption_and_path_bounds(self):
        good = 'Checksums-Sha256:\n ' + 'a'*64 + ' 123 file.tar.xz\nOther: value\n'
        self.assertEqual(sources.source_checksums(good), [('file.tar.xz', 'a'*64)])
        for value in ('', good.replace('123', '-1'), good.replace('file.tar.xz', '../escape'),
                      good.replace('file.tar.xz', '..'), good.replace('a'*64, 'x'*64)):
            with self.assertRaises(ValueError): sources.source_checksums(value)

    def test_t308_source_collection_uses_exact_version_and_checksums(self):
        cache = self.root / 'cache'
        cache.mkdir()
        destination = self.root / 'sources'
        destination.mkdir()
        archive = cache / 'fixture.tar.xz'
        archive.write_bytes(b'fixture')
        descriptor = cache / 'fixture_1.0.dsc'
        descriptor.write_text('Checksums-Sha256:\n ' + tools.digest(archive) + ' 7 fixture.tar.xz\n')
        with patch.object(sources.subprocess, 'run') as command:
            sources.download_debian_source('fixture', '1:1.0', cache, destination)
            self.assertIn('fixture=1:1.0', command.call_args.args[0])
            self.assertEqual((destination / archive.name).read_bytes(), b'fixture')
            archive.write_bytes(b'corrupt')
            with self.assertRaises(ValueError):
                sources.download_debian_source('fixture', '1:1.0', cache, destination)

    def test_t308_crate_sources_preserve_notices(self):
        source = self.root / 'crate'
        source.mkdir()
        self.write(source / 'Cargo.toml', '[package]')
        self.write(source / 'LICENSE', 'copyright fixture')
        self.write(source / 'licenses/extra.txt', 'another license')
        destination = self.root / 'sources'
        destination.mkdir()
        notices = self.root / 'notices'
        notices.mkdir()
        package = dict(id='fixture-id', source='registry', name='fixture', version='1.0', license='MIT', manifest_path=str(source/'Cargo.toml'))
        metadata = dict(resolve=dict(nodes=[dict(id='fixture-id')]), packages=[package])
        with patch.object(sources, 'run', return_value=json.dumps(metadata)):
            records = sources.rust_sources(REPO, destination, notices)
        self.assertEqual(records[0]['name'], 'fixture')
        self.assertEqual((notices / 'fixture-1.0/LICENSE').read_text(), 'copyright fixture')
        self.assertTrue((notices / 'fixture-1.0/licenses/extra.txt').exists())

    def test_t308_package_ownership_and_evdi_provenance(self):
        from types import SimpleNamespace
        with patch.object(sources.subprocess, 'run', return_value=SimpleNamespace(returncode=0, stdout='fixture:amd64: /usr/bin/tool')):
            self.assertEqual(sources.package_for(Path('/usr/bin/tool')), 'fixture:amd64')
        with patch.object(sources.subprocess, 'run', return_value=SimpleNamespace(returncode=1)):
            with self.assertRaises(ValueError): sources.package_for(Path('/usr/bin/missing'))
        with patch.object(sources, 'run', return_value='fixture\t1.0'):
            self.assertEqual(sources.package_info('fixture'), ('fixture', '1.0'))
        with patch.object(sources, 'run', return_value='fixture\t../version'):
            with self.assertRaises(ValueError): sources.package_info('fixture')
        for revision, dirty in [('wrong', ''), ('2713cd41932f2bd8697953a205862a68a966b5ba', ' M tracked')]:
            with patch.object(sources, 'run', side_effect=[revision, dirty]):
                with self.assertRaises(ValueError): sources.verify_evdi(REPO, self.root)
        with patch.object(sources, 'run', side_effect=['2713cd41932f2bd8697953a205862a68a966b5ba', '']):
            self.assertEqual(len(sources.verify_evdi(REPO, self.root)), 40)

    def test_t308_version_argument_bounds(self):
        for version in ('../escape', '', '-1.2.3', '1.2', '1.2.3/dir'):
            with patch.object(sys, 'argv', ['build.py', '--bundle', '/bundle', '--output', '/output', '--version=' + version]):
                with patch('sys.stderr', io.StringIO()), self.assertRaises(SystemExit):
                    build.arguments()
        with patch.object(sys, 'argv', ['build.py', '--bundle', '/bundle', '--output', '/output', '--version=1.2.3']):
            self.assertEqual(build.arguments().version, '1.2.3')

    def test_t308_interrupted_install_preserves_previous_directory(self):
        self.installer_fixture()
        self.assertEqual(self.invoke('--install-user').returncode, 0)
        installed = Path(self.env['XDG_DATA_HOME']) / 'uscreen/appimage/UScreen.AppDir'
        sentinel = installed / 'sentinel'
        sentinel.write_text('previous installation')
        self.write(self.app / 'usr/bin/mv', '#!/bin/sh\ncase "$2" in */.appdir.*) exit 17 ;; esac\nexec /usr/bin/mv "$@"\n')
        result = self.invoke('--install-user')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sentinel.read_text(), 'previous installation')
        self.assertFalse(installed.with_suffix('.AppDir.previous').exists())
        (self.app / 'usr/bin/mv').unlink()
        self.write(self.app / 'usr/bin/cp', '#!/bin/sh\nexit 17\n')
        self.assertNotEqual(self.invoke('--install-user').returncode, 0)
        self.assertEqual(sentinel.read_text(), 'previous installation')
        outer = self.write(self.root / 'image', 'image fixture')
        self.assertNotEqual(self.invoke('--install-user', env=dict(self.env, APPIMAGE=str(outer))).returncode, 0)
        (self.app / 'usr/bin/cp').unlink()
        installed.with_suffix('.AppDir.previous').mkdir()
        result = self.invoke('--install-user')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('interrupted', result.stderr)


if __name__ == '__main__':
    unittest.main()
