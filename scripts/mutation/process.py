"""Bounded owned process trees; platform lifecycle stays in this adapter."""
import os
from pathlib import Path
import signal
import subprocess
import time


def linux_children(pid):
    children = set()
    for path in Path(f'/proc/{pid}/task').glob('*/children'):
        try:
            children.update(int(value) for value in path.read_text().split())
        except FileNotFoundError:
            pass
    return children


def freeze_linux_tree(pid):
    # Cargo and test helpers can create additional sessions/process groups.
    # Freeze each parent before discovering children, then kill leaves first.
    try:
        os.kill(pid, signal.SIGSTOP)
    except ProcessLookupError:
        return []
    descendants = []
    for child in linux_children(pid):
        descendants.extend(freeze_linux_tree(child))
    return descendants + [pid]


def terminate_linux(child):
    for pid in freeze_linux_tree(child.pid):
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def terminate(child):
    if os.name == 'nt':
        subprocess.run(['taskkill', '/PID', str(child.pid), '/T', '/F'],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       timeout=30, check=False)
    else:
        terminate_linux(child)
    child.wait(timeout=30)


def execute(command, directory, environment, log, timeout):
    started = time.monotonic()
    with log.open('wb') as output:
        try:
            child = subprocess.Popen(command, cwd=directory, env=environment,
                                     stdout=output, stderr=subprocess.STDOUT,
                                     start_new_session=os.name != 'nt')
        except OSError as error:
            output.write(str(error).encode())
            return dict(status='tool_error', error=str(error), command=command)
        try:
            code = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            terminate(child)
            return dict(status='timeout', command=command, seconds=time.monotonic()-started)
        except BaseException:
            terminate(child)
            raise
    return dict(status='completed', code=code, command=command,
                seconds=time.monotonic()-started)
