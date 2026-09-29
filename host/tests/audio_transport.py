"""T718 full host CLI / fake authorized tablet / real private PipeWire adapter."""
import json
import os
from pathlib import Path
import select
import signal
import socket
import struct
import subprocess
import threading
import time

ADB = '''#!/usr/bin/env python3
import os,sys
from pathlib import Path
root=Path(os.environ['BLENT_AUDIO_FIXTURE'])
args=sys.argv[1:]
if args[-1:] == ['shell']:
 (root/'invitation').write_text(sys.stdin.read()); print('Broadcast completed: result=1')
elif '--list' in args:
 if (root/'mapping').exists(): print((root/'mapping').read_text())
elif '--remove' in args: (root/'mapping').unlink()
elif 'reverse' in args:
 port=args[-1].split(':')[1]
 (root/'mapping').write_text('Usb tcp:'+port+' tcp:'+port); print(port)
else: print('fixture')
'''


def tablet(root, stop, errors):
    try:
        deadline = time.monotonic() + 5
        while not (root/'invitation').exists():
            assert time.monotonic() < deadline, 'missing invitation'
            time.sleep(.01)
        words = (root/'invitation').read_text().split()
        token = words[words.index('token') + 1]
        port = int(words[words.index('port') + 1])
        with socket.create_connection(('127.0.0.1', port), timeout=2) as peer:
            peer.sendall(b'BLAUREQ1' + token.encode() + bytes([7, 1, 1, 1]))
            grant = peer.makefile('rb').read(156)
            assert len(grant) == 156 and grant[:64] == token.encode()
            generation = struct.unpack('>Q', grant[136:144])[0]
            sequence = 0
            while not stop.wait(.01):
                pcm = struct.pack('<480h', *([2345] * 480))
                peer.sendall(struct.pack('>QQQHBB', generation, sequence, sequence * 10000, len(pcm), 1, 0) + pcm)
                sequence += 1
    except Exception as error:
        errors.append(error)


def nodes(env):
    return json.loads(subprocess.check_output(['pw-dump'], env=env))


def configure(env, source):
    for node in nodes(env):
        name = node.get('info', {}).get('props', {}).get('node.name')
        if name not in (source, 'pw-cat'):
            continue
        direction = 'Output' if name == source else 'Input'
        parameter = '{ direction: '+direction+', mode: dsp, format: { mediaType: audio, mediaSubtype: raw, format: F32P, rate: 48000, channels: 1, position: [ MONO ] } }'
        subprocess.run(['pw-cli', 'set-param', str(node['id']), 'PortConfig', parameter], env=env, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['pw-link', source+':capture_MONO', 'pw-cat:input_MONO'], env=env, check=True)


def check(host, env, directory):
    root = Path(directory)
    (root/'adb').write_text(ADB); (root/'adb').chmod(0o700)
    env = dict(env, PATH=str(root)+os.pathsep+env['PATH'], BLENT_AUDIO_FIXTURE=str(root))
    process = subprocess.Popen([host, 'audio', '--serial', 'fixture'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stop = threading.Event(); errors = []
    worker = threading.Thread(target=tablet, args=(root, stop, errors)); worker.start()
    consumer = None
    try:
        deadline = time.monotonic() + 5
        source = None
        while source is None:
            assert time.monotonic() < deadline, 'host did not publish source'
            source = next((n.get('info', {}).get('props', {}).get('node.name') for n in nodes(env)
                           if n.get('info', {}).get('props', {}).get('node.name', '').startswith('blent_microphone_')), None)
            time.sleep(.02)
        consumer = subprocess.Popen(['pw-cat', '--record', '--target', '0', '--rate', '48000', '--channels', '1', '--format', 's16', '-'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        time.sleep(.1); configure(env, source)
        assert select.select([consumer.stdout], [], [], 3)[0], 'consumer timeout'
        values = consumer.stdout.read(9600)
        assert struct.pack('<h', 2345) in values, 'host lost tablet PCM'
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=3)
        stop.set(); worker.join(timeout=3); assert not worker.is_alive()
        assert process.returncode == 0, stderr.decode()
        assert all(isinstance(error, (BrokenPipeError, ConnectionResetError)) for error in errors), errors
        assert not (root/'mapping').exists(), 'owned reverse mapping leaked'
        assert not any(n.get('info', {}).get('props', {}).get('node.name') == source for n in nodes(env))
    finally:
        stop.set(); worker.join(timeout=3)
        if consumer is not None: consumer.terminate(); consumer.wait(timeout=2)
        if process.poll() is None: process.kill(); process.wait(timeout=2)
