"""T719 private native sink: synthetic stereo, underflow, owned retirement."""
import json
import os
import select
import struct
import subprocess
import sys
import tempfile
import threading
import time


def nodes(env):
    return json.loads(subprocess.check_output(['pw-dump'], env=env))


def connect(env, name):
    for node in nodes(env):
        current = node.get('info', {}).get('props', {}).get('node.name')
        if current not in (name, 'pw-cat'):
            continue
        direction = 'Input' if current == name else 'Output'
        config = '{ direction: '+direction+', mode: dsp, format: { mediaType: audio, mediaSubtype: raw, format: F32P, rate: 48000, channels: 2, position: [ FL FR ] } }'
        subprocess.run(['pw-cli', 'set-param', str(node['id']), 'PortConfig', config], env=env, check=True, stdout=subprocess.DEVNULL)
    for channel in ('FL', 'FR'):
        subprocess.run(['pw-link', 'pw-cat:output_'+channel, name+':playback_'+channel], env=env, check=True)


def feed(producer, errors):
    try:
        block = struct.pack('<960h', *([1234, -4321] * 480))
        for _ in range(100):
            producer.stdin.write(block)
            producer.stdin.flush()
            time.sleep(.01)
    except Exception as error:
        errors.append(error)


def check(helper, env, clients):
    name = 'blent_speakers_t719_private'
    sink = subprocess.Popen([helper, '40', name, 'speakers'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    clients.append(sink)
    assert select.select([sink.stdout], [], [], 5)[0], 'sink readiness timeout'
    assert sink.stdout.readline() == b'READY\n', 'T719 helper did not publish a speaker sink'
    props = next(n['info']['props'] for n in nodes(env) if n.get('info', {}).get('props', {}).get('node.name') == name)
    assert props['media.class'] == 'Audio/Sink'
    producer = subprocess.Popen(['pw-cat', '--playback', '--target', '0', '--rate', '48000', '--channels', '2', '--format', 's16', '-'], env=env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    clients.append(producer)
    time.sleep(.15)
    connect(env, name)
    errors = []
    worker = threading.Thread(target=feed, args=(producer, errors)); worker.start()
    data = bytearray(); deadline = time.monotonic() + 1.5
    while time.monotonic() < deadline:
        if select.select([sink.stdout], [], [], .05)[0]:
            data.extend(os.read(sink.stdout.fileno(), 8192))
    worker.join(timeout=2)
    assert not worker.is_alive() and not errors, errors
    blocks = [data[i:i+1921] for i in range(0, len(data)-1920, 1921)]
    assert len(blocks) > 80
    assert all(b[0] in (0, 1) for b in blocks)
    assert any(struct.pack('<hh', 1234, -4321) in b[1:] for b in blocks), 'stereo/channel order lost'
    assert set(blocks[-1][1:]) == {0}, 'native underflow repeated stale sound'
    sink.stdin.close(); sink.wait(timeout=2)
    assert not any(n.get('info', {}).get('props', {}).get('node.name') == name for n in nodes(env))


def retire(client):
    if client.stdin is not None and not client.stdin.closed:
        client.stdin.close()
    if client.poll() is not None:
        return
    client.terminate()
    try:
        client.wait(timeout=2)
    except subprocess.TimeoutExpired:
        client.kill(); client.wait(timeout=2)


def main(helper, host):
    with tempfile.TemporaryDirectory(prefix='blent-speakers-') as directory:
        env = dict(os.environ, XDG_RUNTIME_DIR=directory, PIPEWIRE_RUNTIME_DIR=directory)
        server = subprocess.Popen(['pipewire'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        clients = []
        try:
            for _ in range(50):
                if os.path.exists(directory+'/pipewire-0'):
                    break
                time.sleep(.02)
            check(helper, env, clients)
            for client in clients:
                retire(client)
            import audio_speaker_transport
            audio_speaker_transport.check(host, env, directory)
            failed = subprocess.Popen([helper, '40', 'blent_speakers_server_loss', 'speakers'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            clients.append(failed)
            assert select.select([failed.stdout], [], [], 5)[0], 'server-loss readiness timeout'
            assert failed.stdout.readline() == b'READY\n'
            retire(server)
            failed.wait(timeout=2)
        finally:
            try:
                for client in clients:
                    retire(client)
            finally:
                retire(server)


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
