"""Read-only samplers for T382. No tokens, screenshots or input coordinates."""
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time


def command(args):
    try:
        result = subprocess.run(args, text=True, capture_output=True, timeout=12)
        return result.returncode, result.stdout.strip(), result.stderr.strip()
    except subprocess.TimeoutExpired:
        return -1, '', 'timeout'


def process_stat(raw, page_size):
    fields = raw.rsplit(') ', 1)[1].split()
    return dict(pid=int(raw.split(' ', 1)[0]), ticks=int(fields[11]) + int(fields[12]),
                start_ticks=int(fields[19]), rss_bytes=int(fields[21]) * page_size,
                threads=int(fields[17]))


def host_process(pid, role):
    try:
        result = process_stat(Path(f'/proc/{pid}/stat').read_text(), os.sysconf('SC_PAGE_SIZE'))
        result['role'] = role
        return result
    except (OSError, ValueError, IndexError):
        return dict(pid=pid, role=role, missing=True)


def service_processes():
    _, group, _ = command(['systemctl', '--user', 'show', 'blent', '-p', 'ControlGroup', '--value'])
    try:
        pids = Path('/sys/fs/cgroup', group.lstrip('/'), 'cgroup.procs').read_text().split()
        return [(int(pid), Path(f'/proc/{pid}/comm').read_text().strip()) for pid in pids]
    except OSError:
        return []


def battery_values(raw):
    names = {'USB powered', 'AC powered', 'status', 'level', 'scale', 'voltage',
             'temperature', 'Charge counter', 'Max charging current', 'Max charging voltage'}
    result = {}
    for line in raw.splitlines():
        key, sep, value = line.strip().partition(':')
        if sep and key in names:
            result[key] = value.strip()
    return result


class Sampler:
    def __init__(self, serial, folder, state, stop, extra_pids, *, android_page_size):
        self.adb = ['adb', '-s', serial]
        self.folder, self.state, self.stop = folder, state, stop
        self.extra_pids = extra_pids
        self.android_page_size = android_page_size
        self.count = 0
        self.failure = None
        self.done = threading.Event()
        self.last_sample = time.monotonic()
        self.file = (folder / 'samples.jsonl').open('w')

    def android(self, args):
        code, out, err = command(self.adb + ['shell'] + args)
        return {'code': code, 'out': out, 'error': err}

    def app_process(self):
        raw = self.android(['p=$(pidof io.github.geraldo_netto.blent); test -n "$p" && run-as io.github.geraldo_netto.blent cat /proc/$p/stat'])
        if raw['code'] != 0:
            return raw
        try:
            return process_stat(raw['out'], self.android_page_size)
        except (ValueError, IndexError):
            return {'error': 'unparseable process stat'}

    def slow_sample(self, result):
        battery = self.android(['dumpsys', 'battery'])
        result['battery'] = battery_values(battery['out'])
        result['battery_error'] = battery['error']
        thermal = self.android(['dumpsys', 'thermalservice'])
        result['thermal'] = [line.strip() for line in thermal['out'].splitlines()
                             if 'Thermal Status:' in line or 'Temperature{' in line]
        display = self.android(['dumpsys', 'display'])
        result['display'] = [line.strip() for line in display['out'].splitlines()
                             if 'mActiveModeId=' in line or 'mBrightnessState=' in line]

    def memory_sample(self, result):
        memory = self.android(['dumpsys', 'meminfo', '--local', 'io.github.geraldo_netto.blent'])
        result['app_memory'] = [line.strip() for line in memory['out'].splitlines()
                                if re.match(r'\s*(TOTAL|Native Heap|Dalvik Heap)', line)]

    def sample(self):
        start = time.monotonic()
        result = dict(utc=time.time(), monotonic=start, **self.state.copy())
        pids = service_processes() + self.extra_pids
        result['host'] = [host_process(pid, role) for pid, role in pids]
        result['loadavg'] = os.getloadavg()
        result['android'] = self.app_process()
        if 'ticks' not in result['android']:
            raise ValueError('Android process sample unavailable')
        if self.count % 6 == 0:
            self.slow_sample(result)
        if self.count % 12 == 0:
            self.memory_sample(result)
        result['collection_seconds'] = time.monotonic() - start
        self.file.write(json.dumps(result) + '\n')
        self.file.flush()
        self.count += 1
        self.last_sample = time.monotonic()

    def run(self):
        try:
            while not self.stop.is_set():
                start = time.monotonic()
                self.sample()
                self.stop.wait(max(0, 5 - (time.monotonic() - start)))
        except Exception as error:
            self.failure = f'sampler failed: {error}'
        finally:
            self.file.close()
            self.done.set()

    def problem(self):
        if self.failure:
            return self.failure
        if self.done.is_set():
            return 'sampler exited before workload completion'
        if time.monotonic() - self.last_sample > 30:
            return 'sampler observations are stale'
        return None


def journal_record(line):
    entry = json.loads(line)
    message = entry.get('MESSAGE', '')
    if isinstance(message, list):
        message = bytes(message).decode('utf-8', errors='replace')
    message = re.sub(r'\x1b\[[0-9;]*m', '', message)
    return {'utc': int(entry['__REALTIME_TIMESTAMP']) / 1e6, 'message': message}


def filtered_logs(args, path, pattern, journal=False):
    return LogCollector(args, path, pattern, journal)


class LogCollector:
    def __init__(self, args, path, pattern, journal):
        self.path, self.pattern, self.journal = path, pattern, journal
        self.failure, self.closing = None, False
        self.receipts = []
        self.process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.thread = threading.Thread(target=self.read, daemon=True)
        self.thread.start()

    def __iter__(self):
        return iter((self.process, self.thread))

    def read(self):
        try:
            self.copy_lines()
            if not self.closing:
                self.failure = f'{self.path.name} collector exited early'
        except Exception as error:
            self.failure = f'{self.path.name} collector failed: {error}'

    def copy_lines(self):
        with self.process.stdout, self.path.open('w') as out:
            for line in self.process.stdout:
                record = journal_record(line) if self.journal else None
                message = record['message'] if self.journal else line
                if not self.pattern.search(message):
                    continue
                self.receipts.append(time.time())
                if self.journal:
                    line = json.dumps(record) + '\n'
                # Defensive redaction if future performance messages add an identifier.
                line = re.sub(r'\b[0-9a-fA-F]{64}\b', '[redacted]', line)
                out.write(line)
                out.flush()

    def problem(self):
        if self.failure:
            return self.failure
        if self.process.poll() is not None or not self.thread.is_alive():
            return f'{self.path.name} collector exited early'
        return None

    def close(self):
        if self.closing:
            return
        self.failure = self.problem()
        self.closing = True
        self.process.terminate()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=3)
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            self.failure = f'{self.path.name} collector did not stop'
