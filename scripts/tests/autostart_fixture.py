"""T536: isolated systemd state and execution of real XDG login entries."""
from pathlib import Path
import subprocess
import time

REPO = Path(__file__).resolve().parents[2]


def manager(root, env, executable):
    state = root / 'manager'
    state.mkdir()
    executable.write_bytes((REPO / 'testdata/autostart_systemctl.sh').read_bytes())
    executable.chmod(0o755)
    env['BLENT_T536_STATE'] = str(state)
    return state


def login(entry, env, starts):
    subprocess.run(['gio', 'launch', str(entry)], env=env, check=True, capture_output=True, timeout=5)
    state = Path(env['BLENT_T536_STATE'])
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        calls = (state / 'calls').read_text().splitlines()
        if calls.count('--user start blent.service') >= starts and (state / 'launches').exists():
            return (state / 'launches').read_text()
        time.sleep(0.01)
    raise AssertionError(f'T536: login did not start the service: {calls}')
