#!/usr/bin/env python3
"""T492: isolate real frame exchange/writer and stock encoding; no tablet actions.

Candidates modify a build-directory copy only. Not a production cadence option.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import time

import profile_usb_pipeline as pipeline
from profile_usb_wire import TeePackets

ROOT = Path(__file__).resolve().parents[2]
VARIANTS = [('current', 200, '1'), ('two', 500, '1'), ('coordinated', 500, '0.9')]


def dump(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def build(folder, interval):
    folder.mkdir()
    original = (ROOT / 'host/evdi/writer.c').read_text()
    old = '#define IDLE_KEEPALIVE_MS 200'
    if original.count(old) != 1:
        raise ValueError('writer policy changed; reassess the experiment')
    (folder / 'candidate_writer.c').write_text(original.replace(old, f'#define IDLE_KEEPALIVE_MS {interval}'))
    command = ['cc', '-std=c11', '-O2', '-pthread', '-Wall', '-Wextra', '-Werror',
               '-I', str(folder), '-I', str(ROOT / 'host/evdi'),
               str(ROOT / 'scripts/benchmarks/idle_writer.c'),
               str(ROOT / 'host/evdi/frame_exchange.c'), str(ROOT / 'host/evdi/fifo_writer.c'),
               '-o', str(folder / 'writer')]
    subprocess.run(command, check=True, timeout=30)
    return command


def collect(process, folder):
    parser, rows = TeePackets(), []
    deadline = time.monotonic() + 15
    with selectors.DefaultSelector() as selector, (folder / 'encoded.h264').open('wb') as output:
        selector.register(process.stdout, selectors.EVENT_READ)
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise TimeoutError('bounded encoder experiment expired')
            for key, _ in selector.select(.25):
                data = os.read(key.fileobj.fileno(), 65536)
                for packet, pts in parser.feed(data):
                    rows.append(dict(pts=pts, ready_us=time.monotonic_ns() // 1000, bytes=len(packet)))
                    output.write(packet)
                if not data:
                    selector.unregister(key.fileobj)
    parser.finish()
    return rows


def stop(process):
    if process.poll() is None:
        process.kill()
    process.wait(timeout=3)


def execute(binary, command, fps, mixed, folder):
    with (folder / 'writer.jsonl').open('wb') as log, (folder / 'ffmpeg.log').open('wb') as errors:
        producer = subprocess.Popen([str(binary), str(fps), str(int(mixed))],
                                    stdout=subprocess.PIPE, stderr=log)
        encoder = None
        try:
            encoder = subprocess.Popen(command, stdin=producer.stdout, stdout=subprocess.PIPE,
                                       stderr=errors, bufsize=0)
            producer.stdout.close()
            rows = collect(encoder, folder)
            if encoder.wait(timeout=3) or producer.wait(timeout=3):
                raise RuntimeError('writer or encoder failed; inspect retained logs')
            return rows
        finally:
            stop(producer)
            if encoder is not None:
                stop(encoder)
                encoder.stdout.close()


def probe(folder, rows):
    command = ['ffprobe', '-v', 'error', '-f', 'h264', '-show_packets', '-show_entries',
               'packet=pos,size,flags', '-of', 'json', str(folder / 'encoded.h264')]
    packets = json.loads(subprocess.check_output(command, text=True, timeout=10))['packets']
    if len(packets) != len(rows):
        raise ValueError('packet count differs from framed observations')
    position = 0
    for row, packet in zip(rows, packets):
        if int(packet['size']) != row['bytes'] or int(packet['pos']) != position:
            raise ValueError('packet boundaries differ')
        row['keyframe'] = 'K' in packet['flags']
        position += row['bytes']
    dump(folder / 'probe.json', dict(command=command, packets=packets))


def independent_keys(folder, packets):
    decoded, offset = 0, 0
    data = (folder / 'encoded.h264').read_bytes()
    for packet in packets:
        payload = data[offset:offset + packet['bytes']]
        offset += packet['bytes']
        if not packet['keyframe']:
            continue
        command = ['ffmpeg', '-v', 'error', '-f', 'h264', '-i', 'pipe:0',
                   '-frames:v', '1', '-f', 'rawvideo', 'pipe:1']
        result = subprocess.run(command, input=payload, capture_output=True, check=True, timeout=10)
        if len(result.stdout) != 1280 * 800 * 3 // 2:
            raise ValueError('keyframe does not decode independently')
        decoded += 1
    return decoded


def summarize(folder, packets):
    events = [json.loads(line) for line in (folder / 'writer.jsonl').read_text().splitlines()
              if line.startswith('{')]
    writes = [event for event in events if event['event'] == 'write']
    if any(row['remaining'] for row in writes) or len(writes) != len(packets):
        raise ValueError('incomplete writer frames or missing final encoded packet')
    keys = [row for row in packets if row['keyframe']]
    if not keys or not packets[0]['keyframe']:
        raise ValueError('missing startup IDR')
    gaps = [(b['ready_us'] - a['ready_us']) / 1000 for a, b in zip(keys, keys[1:])]
    return dict(frames=len(writes), bytes=sum(row['bytes'] for row in packets),
                max_keyframe_gap_ms=max(gaps), keyframe_gaps_ms=gaps,
                independently_decoded_keys=independent_keys(folder, packets),
                final_packet_delay_ms=(packets[-1]['ready_us'] - writes[-1]['start_us']) / 1000,
                publications=publication_delays(events, writes),
                write_to_packet_ms=[(p['ready_us'] - w['start_us']) / 1000
                                    for p, w in zip(packets, writes)])


def publication_delays(events, writes):
    observations = []
    for publication in [row for row in events if row['event'] == 'publish']:
        matches = [row for row in writes if row['marker'] == publication['marker']]
        observations.append(dict(marker=publication['marker'],
                                 delay_ms=(matches[0]['start_us'] - publication['start_us']) / 1000
                                 if matches else None))
    return observations


def trial(folder, binary, profile, fps, mixed, threshold, render_node):
    folder.mkdir()
    command = pipeline.command(profile, dict(width=1280, height=800, fps=fps), render_node)
    index = command.index('-force_key_frames') + 1
    command[index] = f'expr:if(isnan(prev_forced_t),1,gte(t,prev_forced_t+{threshold}))'
    dump(folder / 'command.json', command)
    packets = execute(binary, command, fps, mixed, folder)
    probe(folder, packets)
    dump(folder / 'packets.json', packets)
    result = summarize(folder, packets)
    dump(folder / 'summary.json', result)
    return result


def cases(round_number, policies):
    rows = [(fps, scene, encoder, variant) for fps in [30, 60]
            for scene in [False, True] for encoder in policies for variant in VARIANTS]
    return rows if round_number % 2 == 0 else list(reversed(rows))


def provenance(builds, policies):
    sources = list((ROOT / 'host/evdi').glob('*.[ch]'))
    sources += [Path(__file__), Path(__file__).with_name('idle_writer.c'), Path(pipeline.__file__),
                Path(__file__).with_name('profile_usb_wire.py')]
    executables = [Path(shutil.which(name)).resolve() for name in ['cc', 'ffmpeg', 'ffprobe']]
    return dict(builds=builds, policies=policies, variants=VARIANTS,
         ffmpeg=subprocess.check_output(['ffmpeg', '-version'], text=True),
         revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
         tools={str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in executables},
         sources={str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources})


def run(args):
    args.output.mkdir(parents=True)
    builds = {name: build(args.output / name, interval) for name, interval, _ in VARIANTS}
    policies = {fps: pipeline.policies(fps, 20000, 18) for fps in [30, 60]}
    dump(args.output / 'metadata.json', provenance(builds, policies))
    results = []
    for number in range(args.rounds):
        for fps, mixed, encoder, (name, _, threshold) in cases(number, args.encoders):
            label = f'{number}-{fps}-{int(mixed)}-{encoder}-{name}'
            row = trial(args.output / label, args.output / name / 'writer', policies[fps][encoder],
                        fps, mixed, threshold, args.vaapi_device)
            results.append(dict(label=label, fps=fps, mixed=mixed, encoder=encoder, variant=name, **row))
            dump(args.output / 'results.json', results)
            print(label, row['frames'], round(row['max_keyframe_gap_ms'], 2), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=lambda value: Path(value).resolve())
    parser.add_argument('--rounds', type=int, choices=[1, 2], default=1)
    parser.add_argument('--encoders', nargs='+', choices=['libx264', 'h264_vaapi_baseline'], default=['libx264'])
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128')
    run(parser.parse_args())


if __name__ == '__main__':
    main()
