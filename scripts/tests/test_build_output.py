"""T336: native source workflows must build and consume the same artifact directory."""
from pathlib import Path
import json
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest

import test_notices

REPO = Path(__file__).resolve().parents[2]


def write(root, name, body, executable=False):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)
    if executable:
        path.chmod(0o755)
    return path


def fixture(root, selection, stale):
    version = test_notices.NoticeTest().copy_sources(root)
    tools = root / 'tools'
    tools.mkdir()
    import release_signing_fixture
    release_signing_fixture.install_tools(tools)
    shutil.copy(REPO / 'testdata/cargo_output.py', tools / 'cargo')
    (tools / 'cargo').chmod(0o755)
    write(root, 'tools/gcc', '#!/bin/sh\nmkdir -p host/evdi\nprintf "#!/bin/sh\\necho fresh-helper\\n" > host/evdi/evdi_helper\nchmod +x host/evdi/evdi_helper\n', True)
    for tool in ['sudo', 'systemctl', 'gtk-update-icon-cache', 'kbuildsycoca6']:
        write(root, 'tools/' + tool, '#!/bin/sh\nexit 0\n', True)
    write(root, 'android/gradlew', '#!/bin/sh\nexit 0\n', True)
    write(root, 'android/app/build/outputs/apk/release/app-release.apk', 'fixture-apk')
    write(root, 'fixture.so', 'fixture-library')
    if stale:
        for name in ['blent', 'blent-gui']:
            write(root, 'target/release/' + name, '#!/bin/sh\necho stale\n', True)
    env = dict(os.environ, HOME=str(root / 'home'), PATH=str(tools) + os.pathsep + os.environ['PATH'],
               BLENT_T336_CARGO_LOG=str(root / 'cargo.log'))
    for key in ['CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'MAKEFLAGS', 'MFLAGS', 'CARGO_MAKEFLAGS',
                'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME']:
        env.pop(key, None)
    selected = "cache's space;$(touch injected)"
    if selection == 'absolute':
        env['CARGO_TARGET_DIR'] = str(root / selected)
    elif selection == 'relative':
        env['CARGO_TARGET_DIR'] = selected
    else:
        write(root, '.cargo/config.toml', '[build]\ntarget-dir = ' + json.dumps(selected) + '\n')
    return env, version


class BuildOutputTest(unittest.TestCase):
    def test_t640_linux_ci_checks_private_namespaces_before_collection(self):
        workflow = (REPO / '.github/workflows/build.yml').read_text()
        preflight = 'unshare --user --map-root-user --mount --pid --fork true'
        self.assertIn(preflight, workflow, 'T640: CI must prove private namespace support')
        self.assertLess(workflow.index(preflight), workflow.index('cargo llvm-cov'))
        self.assertIn('sudo apparmor_parser -r testdata/ci-unshare.apparmor', workflow)
        profile = (REPO / 'testdata/ci-unshare.apparmor').read_text()
        self.assertIn('profile unshare /usr/bin/unshare flags=(unconfined)', profile)
        self.assertIn('userns,', profile)

    def test_t639_android_ci_avoids_retired_sdk_tools(self):
        workflow = (REPO / '.github/workflows/build.yml').read_text()
        steps = re.split(r'(?m)^      - ', workflow)
        setup = [step for step in steps if 'uses: android-actions/setup-android@' in step]
        self.assertTrue(setup, 'T639: Android SDK setup must remain in the build')
        for step in setup:
            packages = re.search(r'(?m)^          packages: [\"\']?([^\"\'\n]+)', step)
            self.assertIsNotNone(packages, 'T639: default SDK packages include retired tools')
            selected = packages.group(1).split()
            self.assertNotIn('tools', selected, 'T639: SDK tools package no longer exists')
            self.assertIn('platform-tools', selected, 'T639: retain supported SDK platform tools')

    def run_command(self, root, env, *command):
        result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((root / 'injected').exists(), 'T336: target path evaluated as shell code')
        return result.stdout

    def verify_installed(self, root, env, installed):
        for name in ['blent', 'blent-gui']:
            output = self.run_command(root, env, str(installed / name), 'probe')
            self.assertEqual(output, f'fresh-{name} probe\n', 'T336: stale artifact installed')
        self.assertEqual(self.run_command(root, env, str(installed / 'evdi_helper')), 'fresh-helper\n')

    def test_t336_source_installer_builds_the_artifacts_it_installs(self):
        for selection in ['absolute', 'relative', 'config']:
            for stale in [False, True]:
                with self.subTest(selection=selection, stale=stale), tempfile.TemporaryDirectory() as temp:
                    root = Path(temp) / "source's space;$(touch injected)"
                    root.mkdir()
                    env, _ = fixture(root, selection, stale)
                    source = (root / 'scripts/install.sh').read_text().removesuffix('main "$@"\n')
                    write(root, 'scripts/source-fixture.sh', source + '\nbuild_if_needed\nBIN_DIR="$HOME/installed"\nmkdir -p "$BIN_DIR"\ninstall_binaries\n')
                    self.run_command(root, env, 'bash', 'scripts/source-fixture.sh')
                    self.verify_installed(root, env, root / 'home/installed')
                    built = json.loads((root / 'cargo.log').read_text().splitlines()[-1])[0]
                    self.assertEqual(Path(built), root / 'target')

    def test_t336_make_build_install_run_and_dist_share_outputs(self):
        for selection in ['absolute', 'relative', 'config']:
            with self.subTest(selection=selection), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                env, version = fixture(root, selection, True)
                self.run_command(root, env, 'make', 'install')
                self.verify_installed(root, env, root / 'home/.local/bin')
                for target, argument in [('run', 'start'), ('status', 'status'), ('stop', 'stop'), ('list', 'list-displays')]:
                    output = self.run_command(root, env, 'make', target)
                    self.assertIn(f'fresh-blent {argument}\n', output, 'T336: stale Make action')
                self.run_command(root, env, 'make', 'dist-local', 'LIBEVDI=fixture.so')
                with tarfile.open(root / f'dist/blent-{version}-linux-x86_64.tar.gz') as archive:
                    for name in ['blent', 'blent-gui']:
                        binary = archive.extractfile(f'blent-{version}/bin/{name}').read()
                        self.assertIn(f'fresh-{name}'.encode(), binary, 'T336: stale distribution')
                self.run_command(root, env, 'make', 'clean')
                self.assertFalse((root / 'target').exists(), 'T336: clean targeted a different directory')

    def test_t336_prebuilt_install_uses_bundle_without_cargo(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env, _ = fixture(root, 'relative', True)
            for name in ['blent', 'blent-gui', 'evdi_helper']:
                write(root, 'bin/' + name, '#!/bin/sh\necho prebuilt\n', True)
            installed = root / 'installed'
            self.run_command(root, env, 'bash', 'scripts/install.sh', '--binaries-only', str(installed))
            self.assertEqual(self.run_command(root, env, str(installed / 'blent')), 'prebuilt\n')
            self.assertFalse((root / 'cargo.log').exists())


if __name__ == '__main__':
    unittest.main()
