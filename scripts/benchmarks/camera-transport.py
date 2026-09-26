#!/usr/bin/env python3
"""T608 real kernel TCP/UDP under netem, only inside a new network namespace."""
import argparse
import concurrent.futures
import importlib.util
import json
import os
from pathlib import Path
import resource
import secrets
import socket
import struct
import subprocess
import time
from camera_datagram import Receiver, fragments

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('wire', ROOT / 'camera-socket.py')
WIRE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WIRE)
SCENARIOS = {
    'clean': ['limit', '256'],
    'loss-jitter': ['delay', '12ms', '6ms', 'distribution', 'normal', 'loss', '0.5%',
                    'reorder', '10%', '50%', 'rate', '20mbit', 'limit', '256'],
    'pressure': ['delay', '10ms', '3ms', 'rate', '2mbit', 'limit', '64'],
}


def setup(scenario, parent):
    assert os.readlink('/proc/self/ns/net') != parent, 'refuse live network changes'
    for command in [['ip', 'link', 'set', 'lo', 'up'], ['ip', 'link', 'set', 'lo', 'mtu', '1500'],
                    ['ethtool', '-K', 'lo', 'tso', 'off', 'gso', 'off', 'gro', 'off'],
                    ['tc', 'qdisc', 'add', 'dev', 'lo', 'root', 'netem'] + SCENARIOS[scenario]]:
        subprocess.run(command, check=True, capture_output=True, timeout=5)


def tcp_pair():
    with socket.socket() as listener:
        listener.setsockopt(socket.IPPROTO_TCP, socket.TCP_MAXSEG, 1200)
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        sender = socket.socket()
        sender.settimeout(3)
        sender.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 128*1024)
        sender.setsockopt(socket.IPPROTO_TCP, socket.TCP_MAXSEG, 1200)
        sender.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        sender.connect(listener.getsockname())
        receiver, _ = listener.accept()
        receiver.settimeout(5)
        # Same length/authentication overhead as the existing trusted ADB route.
        greeting = b'BLCAM001' + secrets.token_hex(32).encode() + b'\0\0'
        sender.sendall(greeting)
        with receiver.makefile('rb') as source:
            assert WIRE.exact(source, len(greeting)) == greeting
        receiver.sendall(b'OK')
        assert sender.recv(2) == b'OK'
        return sender, receiver


def udp_pair():
    receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    receiver.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 128*1024)
    receiver.bind(('127.0.0.1', 0))
    sender = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sender.settimeout(3)
    sender.connect(receiver.getsockname())
    return sender, receiver


def receive_tcp(receiver, packets, start):
    rows = []
    with receiver.makefile('rb') as stream:
        for sequence, expected in enumerate(packets):
            size, = struct.unpack('!I', WIRE.exact(stream, 4))
            assert size == len(expected)
            assert WIRE.exact(stream, size) == expected
            rows.append(dict(sequence=sequence, age_ms=(time.monotonic()-start-sequence/30)*1000))
    return dict(arrivals=rows, complete=rows, expired=0, invalid=0, peak_reassembly_bytes=0)


def send(sender, packets, protocol, key, start):
    delays = []
    for sequence, packet in enumerate(packets):
        time.sleep(max(0, start + sequence/30 - time.monotonic()))
        before = time.monotonic()
        if protocol == 'tcp':
            frame = struct.pack('!I', len(packet)) + packet
            for offset in range(0, len(frame), 8192):
                sender.sendall(frame[offset:offset+8192])
        else:
            for part in fragments(key, sequence, packet):
                sender.send(part)
        delays.append((time.monotonic()-before)*1000)
    return delays


def replay(packets, keys, protocol):
    sender, receiver = tcp_pair() if protocol == 'tcp' else udp_pair()
    key, start = secrets.token_bytes(32), time.monotonic() + .1
    reader = Receiver(packets, keys, key, start)
    cpu = time.process_time()
    with sender, receiver, concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        future = pool.submit(receive_tcp, receiver, packets, start) if protocol == 'tcp' else pool.submit(reader.receive, receiver)
        writes = send(sender, packets, protocol, key, start)
        result = future.result(timeout=10)
    result.update(cpu_seconds=time.process_time()-cpu, elapsed_seconds=time.monotonic()-start,
                  max_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, write_ms=writes)
    return result


def run(args):
    setup(args.scenario, args.parent_netns)
    packets = WIRE.fixture(args.fixture)
    records = json.loads(args.fixture.with_name('packets.json').read_text())['packets']
    keys = {i for i, p in enumerate(records) if 'K' in p['flags']}
    result = replay(packets, keys, args.protocol)
    result.update(protocol=args.protocol, scenario=args.scenario, frames=len(packets),
                  netem=SCENARIOS[args.scenario], tcp_mss=1200, udp_payload=1152,
                  qdisc=subprocess.check_output(['tc', '-s', 'qdisc', 'show', 'dev', 'lo'], text=True))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(args.protocol, args.scenario, len(result['arrivals']), '/', len(packets), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--protocol', choices=['tcp', 'udp'], required=True)
    parser.add_argument('--scenario', choices=SCENARIOS, required=True)
    parser.add_argument('--parent-netns', required=True)
    run(parser.parse_args())


if __name__ == '__main__':
    main()
