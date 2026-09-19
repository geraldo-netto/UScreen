#!/usr/bin/env python3
"""Model the runtime's per-invocation extraction and cleanup, without FUSE."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

source = Path(os.environ['USCREEN_TEST_TEMPLATE'])
with tempfile.TemporaryDirectory(prefix='uscreen-runtime-') as root:
    app = Path(root) / 'AppDir'
    shutil.copytree(source, app)
    env = dict(os.environ, APPIMAGE=os.path.realpath(__file__))
    result = subprocess.run([app / 'AppRun', *sys.argv[1:]], env=env)
    raise SystemExit(result.returncode)
