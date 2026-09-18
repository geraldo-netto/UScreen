#!/usr/bin/env python3
"""T400: refine quantizers against three objective H.264 quality floors."""
import argparse
import importlib.util
import json
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('codec_host', Path(__file__).with_name('codec-host.py'))
HOST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HOST)


def acceptable(row, reference, tolerance):
    # A zero-MSE/null-PSNR result is lossless, not a missing observation.
    for key, threshold in reference['psnr_db'].items():
        measured = row['psnr_db'][key]
        if measured is not None and (threshold is None or measured < threshold - tolerance):
            return False
    return True


def validate_measurement(row, result, meta, scene):
    # T462: legacy cached encoder success does not establish clean decoding.
    diagnostics = result.with_name('decode.log')
    clean_decode = diagnostics.is_file() and not diagnostics.read_text().strip()
    matching_source = row.get('reference_sha256') == scene['sha256']
    complete_decode = row.get('decoded_frames') == meta['frames']
    if not (clean_decode and matching_source and complete_decode):
        raise ValueError(f'Unverified codec cache {result}; run a fresh measurement '
                         'with matching corpus metadata and retain decode.log')


def measure(args, meta, scene, encoder, quantizer):
    name = f'{scene["scene"]}-{encoder}-q{quantizer}'
    original = args.sweep / name / 'result.json'
    result = original if original.exists() else args.output / name / 'result.json'
    if not result.exists():
        HOST.trial(args, meta, scene, encoder, quantizer)
    row = json.loads(result.read_text())
    if row['returncode'] != 0:
        raise RuntimeError(f'encoder failed: {result}')
    validate_measurement(row, result, meta, scene)
    return row, str(result)


def select(args, meta, scene, encoder, reference):
    low, high = 0, 51 if encoder in HOST.ENCODERS[:4] else 63
    observations = {}
    while low <= high:
        quantizer = (low + high) // 2
        row, path = measure(args, meta, scene, encoder, quantizer)
        passed = acceptable(row, reference, args.tolerance)
        observations[quantizer] = dict(path=path, passed=passed, result=row)
        if passed:
            low = quantizer + 1
        else:
            high = quantizer - 1
    passing = [q for q, item in observations.items() if item['passed']]
    chosen = max(passing) if passing else None
    return dict(scene=scene['scene'], encoder=encoder, selected_quantizer=chosen,
                selection=observations.get(chosen), observations=observations)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--sweep', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--tolerance', type=float, default=0.5)
    parser.add_argument('--vaapi-device', default='/dev/dri/renderD128')
    args = parser.parse_args()
    args.output.mkdir()
    meta = json.loads((args.corpus / 'metadata.json').read_text())
    result = dict(tolerance_db=args.tolerance, baseline='h264_vaapi q18',
                  method='Binary refinement; highest tested passing quantizer. Assumes approximate monotonicity, '
                         'not a proof of globally optimal quantizer or perceptual equivalence.', selections=[])
    for scene in meta['scenes']:
        reference, _ = measure(args, meta, scene, 'h264_vaapi', 18)
        for encoder in HOST.ENCODERS:
            selected = select(args, meta, scene, encoder, reference)
            result['selections'].append(selected)
            (args.output / 'selection.json').write_text(json.dumps(result, indent=2) + '\n')
            print('SELECTED', scene['scene'], encoder, selected['selected_quantizer'], flush=True)


if __name__ == '__main__':
    main()
