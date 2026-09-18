"""Content and provenance checks shared by codec measurement and replay tools."""
import hashlib
import json
from pathlib import Path
import re


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def verify_file(path, sha256, size):
    if not isinstance(sha256, str) or not re.fullmatch('[0-9a-f]{64}', sha256):
        raise ValueError(f'Missing or invalid SHA-256 for {path}; run a fresh measurement')
    if not isinstance(size, int) or size <= 0 or path.stat().st_size != size:
        raise ValueError(f'Artifact byte count changed: {path}; run a fresh measurement')
    if digest(path) != sha256:
        raise ValueError(f'Artifact contents changed: {path}; run a fresh measurement')
    return path


def raw(corpus, meta, scene):
    width, height, frames = meta['width'], meta['height'], meta['frames']
    if meta['pixel_format'] != 'nv12' or min(width, height, frames, meta['fps']) <= 0:
        raise ValueError('Invalid NV12 corpus format')
    if width % 2 or height % 2:
        raise ValueError('NV12 corpus dimensions must be even')
    expected = width * height * 3 // 2 * frames
    if scene.get('bytes') != expected:
        raise ValueError('Corpus size metadata does not match format; run a fresh measurement')
    return verify_file(corpus / scene['path'], scene.get('sha256'), expected)


def reference_format(meta):
    return {key: meta[key] for key in ['width', 'height', 'frames', 'fps', 'pixel_format']}


def recorded_format(row):
    if 'reference_format' in row:
        return row['reference_format']
    # Historical rows retain the original command; do not rewrite their metadata.
    command = row.get('command', [])
    try:
        value = lambda flag: command[command.index(flag) + 1]
        width, height = map(int, value('-video_size').split('x'))
        return dict(width=width, height=height, frames=int(value('-frames:v')),
                    fps=int(value('-framerate')), pixel_format=value('-pixel_format'))
    except (ValueError, IndexError) as error:
        raise ValueError('Missing corpus format provenance; run a fresh measurement') from error


def measurement(row, result, corpus, meta, scene):
    raw(corpus, meta, scene)
    if recorded_format(row) != reference_format(meta):
        raise ValueError('Corpus format provenance differs; run a fresh measurement')
    diagnostics = result.with_name('decode.log')
    if not diagnostics.is_file() or diagnostics.read_text().strip():
        raise ValueError(f'Unverified decode at {result}; run a fresh measurement and retain decode.log')
    expected = (0, scene['scene'], scene['sha256'], meta['frames'])
    observed = tuple(row.get(key) for key in ['returncode', 'scene', 'reference_sha256', 'decoded_frames'])
    if observed != expected:
        raise ValueError(f'Unverified codec cache {result}; run a fresh measurement')
    stream = row.get('stream', {})
    if (stream.get('width'), stream.get('height')) != (meta['width'], meta['height']):
        raise ValueError(f'Codec geometry changed at {result}; run a fresh measurement')
    return verify_file(result.with_name('encoded.mkv'), row.get('sha256'), row.get('encoded_bytes'))


def identity(row, scene, encoder, quantizer):
    if (row.get('scene'), row.get('encoder'), row.get('quality')) != (scene, encoder, quantizer):
        raise ValueError('Selected codec identity differs from measurement; run a fresh measurement')


def selection(selected, corpus, meta):
    chosen = selected['selection']
    result = Path(chosen['path'])
    row = json.loads(result.read_text())
    if row != chosen['result']:
        raise ValueError(f'Selected result changed: {result}; run a fresh measurement')
    identity(row, selected['scene'], selected['encoder'], selected['selected_quantizer'])
    scenes = {scene['scene']: scene for scene in meta['scenes']}
    source = measurement(row, result, corpus, meta, scenes[selected['scene']])
    return row, source
