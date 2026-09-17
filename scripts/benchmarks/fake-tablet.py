#!/usr/bin/env python3
"""T408: isolated real-socket receiver replay; no daemon, EVDI or real tablet.

CPU trials disable tracemalloc. Separate memory trials record traced peaks.
Frames contain synthetic bytes; ACKs measure receipt, not decoding/rendering.
"""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import multiprocessing
import os
from pathlib import Path
import platform
import resource
import socket
import struct
import subprocess
import tempfile
import threading
import time
import tracemalloc

ROOT = Path(__file__).resolve().parents[2]
WORKLOADS = {'large': (2 * 1024 * 1024, 4096), 'tiny_fragments': (512 * 1024, 64)}


class FragmentLimit:
    def __init__(self, source, fragment):
        self.source, self.fragment = source, fragment
        self.calls = 0
        self.largest_request = 0

    def requested(self, size):
        self.calls += 1
        self.largest_request = max(self.largest_request, size)
        return min(size, self.fragment)

    def recv(self, size):
        return self.source.recv(self.requested(size))

    def recv_into(self, buffer):
        count = self.requested(len(buffer))
        return self.source.recv_into(buffer[:count])


class AckSink:
    def __init__(self):
        self.count = 0

    def sendall(self, _data):
        self.count += 1


def load_module(path):
    spec = importlib.util.spec_from_file_location('receiver_under_test', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def send_workload(output, payload, frames):
    with output:
        output.sendall(struct.pack('>I', 4) + b'\x00csd')
        for sequence in range(frames):
            output.sendall(struct.pack('>IBI', len(payload) + 5, 1, sequence))
            output.sendall(payload)


def measure(source, workload, frames, trace, ready):
    module = load_module(source)
    size, fragment = WORKLOADS[workload]
    receiver, sender = socket.socketpair()
    receiver.settimeout(30)
    limited, control = FragmentLimit(receiver, fragment), AckSink()
    producer = threading.Thread(target=send_workload, args=(sender, bytes(size), frames))
    ready.wait(timeout=30)
    if trace:
        tracemalloc.start()
    usage = resource.getrusage(resource.RUSAGE_SELF)
    start, cpu = time.perf_counter_ns(), time.thread_time_ns()
    producer.start()
    with receiver, contextlib.redirect_stdout(io.StringIO()):
        result = module.receive_video(limited, control, 60)
    elapsed, cpu = time.perf_counter_ns() - start, time.thread_time_ns() - cpu
    producer.join()
    peak = tracemalloc.get_traced_memory()[1] if trace else None
    if trace:
        tracemalloc.stop()
    after = resource.getrusage(resource.RUSAGE_SELF)
    assert result == (True, frames, frames - 1), result
    assert control.count == frames, control.count
    return dict(bytes=size * frames, wall_ns=elapsed, receiver_cpu_ns=cpu,
                traced_peak_bytes=peak, max_rss_kib=after.ru_maxrss,
                voluntary_switches=after.ru_nvcsw - usage.ru_nvcsw,
                involuntary_switches=after.ru_nivcsw - usage.ru_nivcsw,
                recv_calls=limited.calls, largest_request=limited.largest_request)


def worker(queue, *args):
    try:
        queue.put(measure(*args))
    except BaseException as error:
        queue.put({'error': repr(error)})


def run_group(source, clients, workload, frames, trace):
    context = multiprocessing.get_context('spawn')
    queue, ready = context.Queue(), context.Barrier(clients)
    processes = [context.Process(target=worker, args=(queue, source, workload, frames, trace, ready))
                 for _ in range(clients)]
    for process in processes:
        process.start()
    try:
        results = [queue.get(timeout=90) for _ in processes]
    finally:
        for process in processes:
            process.join(timeout=5)
            if process.is_alive():
                process.kill()
                process.join()
        queue.close()
    assert all('error' not in result for result in results), results
    return results


def trials(source, variant, repeats, frames):
    for workload in WORKLOADS:
        for clients in [1, 2, 4]:
            for trial in range(repeats + 1):
                trace = trial == repeats
                results = run_group(str(source), clients, workload, frames, trace)
                yield dict(variant=variant, workload=workload, clients=clients,
                           trial=trial, tracing=trace, results=results)


def host_metadata():
    model = next(line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines()
                 if line.startswith('model name'))
    return dict(cpu_model=model, cpu_affinity=sorted(os.sched_getaffinity(0)),
                working_tree_parent=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-ref', default='d8141d5')
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--frames', type=int, default=8)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    baseline = subprocess.check_output(['git', 'show', f'{args.baseline_ref}:scripts/fake-tablet.py'], cwd=ROOT)
    candidate = ROOT / 'scripts/fake-tablet.py'
    metadata = dict(**host_metadata(), baseline=args.baseline_ref, python=platform.python_version(),
                    platform=platform.platform(), frames=args.frames,
                    baseline_sha256=hashlib.sha256(baseline).hexdigest(),
                    candidate_sha256=hashlib.sha256(candidate.read_bytes()).hexdigest(),
                    ack='receive only; synthetic decode_us=1000; no decode/render',
                    workloads=WORKLOADS)
    with tempfile.TemporaryDirectory(prefix='uscreen-t408-') as temporary:
        old = Path(temporary) / 'baseline.py'
        old.write_bytes(baseline)
        records = []
        for variant, source in [('baseline', old), ('candidate', candidate)]:
            for trial in trials(source, variant, args.repeats, args.frames):
                records.append(trial)
                print(f"{variant} {trial['workload']} {trial['clients']} clients trial {trial['trial']}", flush=True)
                args.output.write_text(json.dumps(dict(metadata=metadata, trials=records), indent=2) + '\n')


if __name__ == '__main__':
    main()
