#!/usr/bin/env python3
"""T607 byte-exact H264 replay receiver; run under strace for isolated counts."""
import argparse
import hashlib
import json
from pathlib import Path
import resource
import socket
import struct
import time


def exact(stream, size):
    data = stream.read(size)
    if len(data) != size:
        raise EOFError('truncated replay')
    return data


def fixture(path):
    with path.open('rb') as source:
        count, = struct.unpack('!I', exact(source, 4))
        packets = []
        for _ in range(count):
            size, = struct.unpack('!I', exact(source, 4))
            assert 0 < size <= 2 * 1024 * 1024
            packets.append(exact(source, size))
        assert not source.read(1)
    return packets


def receive(connection, packets):
    with connection, connection.makefile('rb') as stream:
        chunk, trial, count = struct.unpack('!III', exact(stream, 12))
        assert count == len(packets) * 10
        connection.sendall(struct.pack('!I', count))
        start, cpu = time.monotonic_ns(), time.process_time_ns()
        for index in range(count):
            size, = struct.unpack('!I', exact(stream, 4))
            expected = packets[index % len(packets)]
            assert size == len(expected)
            assert exact(stream, size) == expected
        connection.sendall(struct.pack('!I', count))
        return dict(chunk=chunk, trial=trial, packets=count, elapsed_ns=time.monotonic_ns()-start,
                    cpu_ns=time.process_time_ns()-cpu)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    packets = fixture(args.fixture)
    rows = []
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        listener.settimeout(60)
        (args.output / 'port').write_text(str(listener.getsockname()[1]))
        for _ in range(40):
            connection, _ = listener.accept()
            connection.settimeout(20)
            rows.append(receive(connection, packets))
    result = dict(rows=rows, max_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                  fixture_sha256=hashlib.sha256(args.fixture.read_bytes()).hexdigest())
    (args.output / 'host.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
