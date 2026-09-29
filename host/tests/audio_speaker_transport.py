"""T719 full CLI, authorized fake tablet and private native stereo sink."""
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import threading
import time
import audio_speakers
from audio_transport import ADB, nodes


def tablet(root, received, errors):
    try:
        deadline = time.monotonic() + 5
        while not (root/'invitation').exists():
            assert time.monotonic() < deadline, 'missing speaker invitation'
            time.sleep(.01)
        words = (root/'invitation').read_text().split()
        token = words[words.index('token') + 1]
        assert words[words.index('direction') + 1] == '2'
        port = int(words[words.index('port') + 1])
        with socket.create_connection(('127.0.0.1', port), timeout=3) as peer:
            peer.sendall(b'BLAUREQ1' + token.encode() + bytes([7, 0, 2, 1]))
            stream = peer.makefile('rb')
            grant = stream.read(156)
            assert grant[:64] == token.encode() and grant[144:146] == bytes([2, 2])
            generation = struct.unpack('>Q', grant[136:144])[0]
            consume(stream, generation, received)
    except Exception as error:
        errors.append(error)


def consume(stream, generation, received):
    previous = None
    while packet := stream.read(1948):
        assert len(packet) == 1948, 'partial speaker packet'
        gen, sequence, timestamp, size, channels, reserved = struct.unpack('>QQQHBB', packet[:28])
        assert (gen, size, channels, reserved) == (generation, 1920, 2, 0)
        if previous is None:
            assert sequence == 0
        else:
            assert sequence > previous[0] and timestamp > previous[1]
        previous = sequence, timestamp
        if struct.pack('<hh', 1234, -4321) in packet[28:]:
            received.set()


def wait_sink(env):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        for node in nodes(env):
            name = node.get('info', {}).get('props', {}).get('node.name', '')
            if name.startswith('blent_speakers_'):
                return name
        time.sleep(.02)
    raise AssertionError('host did not publish speaker sink')


def check(host, env, directory):
    root = Path(directory)
    (root/'adb').write_text(ADB); (root/'adb').chmod(0o700)
    env = dict(env, PATH=str(root)+os.pathsep+env['PATH'], BLENT_AUDIO_FIXTURE=str(root))
    process = subprocess.Popen([host, 'audio', '--direction', 'speakers', '--serial', 'fixture'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    received = threading.Event(); errors = []
    worker = threading.Thread(target=tablet, args=(root, received, errors)); worker.start()
    producer = None
    try:
        sink = wait_sink(env)
        producer = subprocess.Popen(['pw-cat', '--playback', '--target', '0', '--rate', '48000', '--channels', '2', '--format', 's16', '-'], env=env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        time.sleep(.15); audio_speakers.connect(env, sink)
        feeder = threading.Thread(target=audio_speakers.feed, args=(producer, errors)); feeder.start()
        assert received.wait(3), errors
        feeder.join(timeout=2); assert not feeder.is_alive()
        process.send_signal(signal.SIGINT)
        _, stderr = process.communicate(timeout=3)
        worker.join(timeout=3); assert not worker.is_alive() and not errors, errors
        assert process.returncode == 0, stderr.decode()
        assert not (root/'mapping').exists(), 'speaker reverse mapping leaked'
        assert not any(n.get('info', {}).get('props', {}).get('node.name') == sink for n in nodes(env))
    finally:
        if producer is not None:
            audio_speakers.retire(producer)
        audio_speakers.retire(process)
        worker.join(timeout=4)
