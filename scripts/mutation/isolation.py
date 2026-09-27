"""T652: native process commands and Linux namespaces. Copyright 2026 Geraldo Netto."""
import os
from pathlib import Path
import shutil
import sys


MEASUREMENT_ENV = ['COVERAGE_PROCESS_START', 'COVERAGE_FILE', 'BLENT_PYTHON_CALLS',
                   'BLENT_COVERAGE_ROOT', 'BLENT_COVERAGE_MANIFEST', 'BASH_ENV',
                   'BLENT_SHELL_COVERAGE_DIR', 'BLENT_SHELL_COVERAGE_ORIGINS',
                   'BLENT_SHELL_COVERAGE_PYTHON', 'PYTHONPATH']


def copy_cache(source, destination):
    if source.is_dir():
        shutil.copytree(source, destination, ignore=shutil.ignore_patterns('*.lock', '*.lck'))


def prepare(output, selected, environment):
    environment = {key: value for key, value in environment.items() if key not in MEASUREMENT_ENV}
    if sys.platform != 'linux':
        return environment
    home = output / 'home'
    home.mkdir()
    value = dict(environment, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
                 XDG_CACHE_HOME=str(home / '.cache'), XDG_DATA_HOME=str(home / '.local/share'),
                 RUSTUP_HOME=os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup')))
    if any('cargo' in command for command in [selected['build'], selected['test']]):
        cargo = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo'))
        value['CARGO_HOME'] = str(home / '.cargo')
        copy_cache(cargo / 'registry', home / '.cargo/registry')
    if selected.get('cache') == 'gradle':
        gradle = Path(os.environ.get('GRADLE_USER_HOME', Path.home() / '.gradle'))
        for name in ['caches', 'wrapper']:
            copy_cache(gradle / name, home / '.gradle' / name)
        copy_cache(Path.home() / '.m2', home / '.m2')
        value['GRADLE_USER_HOME'] = str(home / '.gradle')
        value['JAVA_TOOL_OPTIONS'] = environment.get('JAVA_TOOL_OPTIONS', '') + ' -Duser.home=' + str(home)
    return value


def command(argv, output):
    if sys.platform != 'linux':
        return argv
    return ['bwrap', '--unshare-user', '--unshare-pid', '--unshare-net',
            '--ro-bind', '/', '/', '--dev', '/dev', '--proc', '/proc', '--tmpfs', '/tmp',
            '--bind', str(output), str(output),
            *[part for key in MEASUREMENT_ENV for part in ['--unsetenv', key]], '--', *argv]
