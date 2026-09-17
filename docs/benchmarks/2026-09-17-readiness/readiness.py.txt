#!/usr/bin/env python3
"""T405: build exact baseline/candidate FIFO reader/writer sources and replay."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BASE = '48e9427'
FILES = ['host/src/encoder_io.rs', 'host/src/encoder_fifo.rs',
         'host/evdi/fifo_writer.c', 'host/evdi/fifo_writer.h']
MANIFEST = '''[package]
name = "uscreen-readiness-bench"
version = "0.0.0"
edition = "2021"
[dependencies]
libc = "0.2"
tracing = "0.1"
serde_json = "1"
tempfile = "3"
'''
ADAPTER = '''
pub struct StopSignal(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl StopSignal {
    pub fn new() -> std::io::Result<std::sync::Arc<Self>> { Ok(std::sync::Arc::new(Self(Default::default()))) }
    pub fn request(&self) { self.0.store(true, std::sync::atomic::Ordering::Relaxed); }
}
pub struct FifoReader<T>(T);
impl<T> FifoReader<T> { pub fn new(source: T) -> Self { Self(source) } }
impl<T: std::io::Read> std::io::Read for FifoReader<T> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> { self.0.read(bytes) }
}
pub fn read_frame<T: std::io::Read>(source: &mut T, bytes: &mut [u8], stop: &StopSignal) -> std::io::Result<bool> {
    baseline_read_frame(source, bytes, &stop.0)
}
'''


def source(variant, name):
    candidate = variant in ['candidate', 'coalesced'] or (variant == 'reader-only' and '/src/' in name) or (variant == 'writer-only' and '/evdi/' in name)
    if candidate:
        return (ROOT / name).read_bytes()
    return subprocess.check_output(['git', 'show', f'{BASE}:{name}'], cwd=ROOT)


def prepare(folder, variant):
    folder.mkdir(parents=True)
    (folder / 'src').mkdir()
    for name in FILES:
        if variant in ['baseline', 'writer-only'] and name.endswith('encoder_fifo.rs'):
            continue
        (folder / Path(name).name).write_bytes(source(variant, name))
    original = (folder / 'encoder_io.rs').read_text().split('/// Length of the Annex B prefix')[0]
    reader = original[original.index('\n\n') + 2:]
    if variant in ['baseline', 'writer-only']:
        reader = reader.replace('fn read_frame(', 'fn baseline_read_frame(') + ADAPTER
    else:
        reader = reader.replace('#[cfg(feature = "inproc-encoder")]\n', '')
    (folder / 'src/reader.rs').write_text(reader)
    if variant in ['candidate', 'reader-only', 'coalesced']:
        shutil.copy2(folder / 'encoder_fifo.rs', folder / 'src/encoder_fifo.rs')
    if variant == 'coalesced':
        coalesced(folder)
    for name, target in [('readiness.rs', 'src/main.rs'), ('readiness.c', 'bridge.c')]:
        shutil.copy2(ROOT / 'scripts/benchmarks' / name, folder / target)
    (folder / 'Cargo.toml').write_text(MANIFEST)
    (folder / 'build.rs').write_text('''fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    assert!(std::process::Command::new("cc").args(["-O3", "-Wall", "-Wextra", "-Werror", "-c", "bridge.c", "-o"]).arg(format!("{out}/bridge.o")).status().unwrap().success());
    assert!(std::process::Command::new("ar").arg("rcs").arg(format!("{out}/libbridge.a")).arg(format!("{out}/bridge.o")).status().unwrap().success());
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=bridge");
    println!("cargo:rerun-if-changed=bridge.c");
    println!("cargo:rerun-if-changed=fifo_writer.c");
}
''')


def coalesced(folder):
    reader = folder / 'src/reader.rs'
    text = reader.read_text().replace('    Data,', '    Data,\n    Partial,')
    text = text.replace('if !fifo.wait(waiting, stop)? {', 'let waiting = if waiting == Waiting::Data && filled > 0 { Waiting::Partial } else { waiting };\n                if !fifo.wait(waiting, stop)? {')
    reader.write_text(text)
    fifo = folder / 'src/encoder_fifo.rs'
    text = fifo.read_text().replace('    source: T,', '    source: T,\n    progress: bool,')
    text = text.replace('Self { source, events }', 'Self { source, events, progress: false }')
    text = text.replace('waiting == Waiting::Data', 'waiting != Waiting::Writer')
    text = text.replace('self.source.read(bytes)', 'let result = self.source.read(bytes);\n        if matches!(result, Ok(count) if count > 0) { self.progress = true; }\n        result')
    text = text.replace('        let mut descriptors = self.descriptors(stop, waiting);', '        let progress = std::mem::take(&mut self.progress);\n        if waiting == Waiting::Partial && progress {\n            return poll_ready(&mut [poll_descriptor(stop.event.as_raw_fd())], 2, stop);\n        }\n        let mut descriptors = self.descriptors(stop, waiting);')
    fifo.write_text(text)


def build(folder, variant):
    prepare(folder, variant)
    # Distinct target directories prevent stale cross-revision artifacts.
    env = dict(os.environ, CARGO_TARGET_DIR=str(folder / 'target'))
    subprocess.run(['cargo', 'build', '--offline', '--release'], cwd=folder, env=env, check=True)
    binary = folder / 'readiness'
    shutil.copy2(folder / 'target/release/uscreen-readiness-bench', binary)
    return binary


def cases(frames, control):
    if control:
        yield from ((size, sessions, frames, 'active') for size, sessions in itertools.product([1536000, 8205120], [1, 4]))
        return
    yield from ((size, sessions, frames, 'active') for size, sessions in
                itertools.product([1536000, 8205120], [1, 2, 4]))
    yield from ((16, sessions, 0, mode) for sessions, mode in
                itertools.product([1, 2, 4], ['no-writer', 'empty']))


def run(args, binaries):
    result = dict(baseline=BASE, platform=platform.platform(), affinity=sorted(os.sched_getaffinity(0)),
                  compiler=subprocess.check_output(['rustc', '--version'], text=True).strip(),
                  binary_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in binaries.items()}, trials=[])
    assert len(set(result['binary_sha256'].values())) == len(binaries)
    for number, (size, sessions, frames, mode) in enumerate(cases(args.frames, args.control)):
        for trial in range(args.trials):
            order = list(binaries) if (number + trial) % 2 else list(reversed(binaries))
            for variant in order:
                command = [str(binaries[variant]), str(sessions), str(size), str(frames), mode]
                row = json.loads(subprocess.check_output(command, text=True, timeout=60))
                row.update(variant=variant, trial=trial, size=size, sessions=sessions, frames=frames, mode=mode)
                result['trials'].append(row)
                args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(f'T405 readiness: completed case {number + 1}', flush=True)


def variants_for(args):
    if args.coalesced:
        args.control = True
        return ['baseline', 'candidate', 'coalesced']
    if args.control:
        return ['baseline', 'candidate', 'reader-only', 'writer-only']
    return ['baseline', 'candidate']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--frames', type=int, default=120)
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--reuse', action='store_true')
    parser.add_argument('--control', action='store_true')
    parser.add_argument('--coalesced', action='store_true')
    args = parser.parse_args()
    if args.trials < 1 or not 1 <= args.frames <= 4096:
        parser.error('trials must be positive; frames must be in 1..4096')
    variants = variants_for(args)
    binaries = {name: args.directory / name / 'readiness' for name in variants}
    if not args.reuse:
        binaries = {name: build(args.directory / name, name) for name in binaries}
    if not args.build_only:
        run(args, binaries)


if __name__ == '__main__':
    main()
