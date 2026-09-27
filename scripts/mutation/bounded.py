#!/usr/bin/env python3
"""T652: reviewed cross-language fault campaigns, never a whole-project score.

Copyright (c) 2026 Geraldo Netto.
"""
import argparse
from collections import Counter
import difflib
import json
import os
from pathlib import Path
import re
import sys
import time
import xml.etree.ElementTree as ET

import isolation
from process import execute
from run import ROOT, environment, fingerprint, positive, snapshot, unchanged, write_json


def catalog(path, name, platform):
    data = json.loads(path.read_text())
    if data['version'] != 1:
        raise ValueError('Unknown catalog version')
    selected = data['profiles'][name]
    if platform not in selected['platforms']:
        raise ValueError(f'{name} requires native {selected["platforms"]}; got {platform}')
    candidates = [row for row in data['mutations'] if row['profile'] == name]
    for row in candidates:
        if not re.fullmatch(r'[A-Za-z0-9_-]+', row['id']) or not row['expected_failure']:
            raise ValueError('Candidate needs a safe id and explicit failure assertion')
    ids = [row['id'] for row in candidates]
    if not ids or len(set(ids)) != len(ids):
        raise ValueError('Campaign requires a nonempty unique candidate inventory')
    return selected, candidates


def edited(source, row):
    relative = Path(row['file'])
    if relative.is_absolute() or '..' in relative.parts:
        raise ValueError('Candidate must be inside the private source tree')
    path = source / relative
    if path.is_symlink() or not path.resolve().is_relative_to(source.resolve()):
        raise ValueError('Mutation symlink escapes source ownership')
    with path.open(encoding='utf-8', newline='') as stream:
        original = stream.read()
    before, after = row['before'], row['after']
    if not before or before == after or original.count(before) != 1:
        raise ValueError(f'{row["id"]}: expected exactly one changed source span')
    return path, original, original.replace(before, after, 1)


def invocation(command, source, output):
    substitutions = {'python': sys.executable, 'source': str(source), 'output': str(output)}
    return [argument.format_map(substitutions) for argument in command]


def phase(command, source, output, env, log, timeout, deadline):
    remaining = min(timeout, deadline - time.monotonic())
    if remaining <= 0:
        return dict(status='timeout', command=command, reason='campaign deadline')
    return execute(isolation.command(command, output), source, env, output / log, remaining)


def passed(result):
    return result['status'] == 'completed' and result['code'] == 0


def junit(source, pattern):
    paths = list(source.glob(pattern))
    if not paths:
        return 0, []
    suites = [ET.parse(path).getroot() for path in paths]
    cases = [case for suite in suites for case in suite.iter('testcase')]
    executed = [case for case in cases if case.find('skipped') is None]
    failures = [case.attrib['name'] for case in executed if case.find('failure') is not None]
    return len(executed), failures


def test_evidence(source, selected, log):
    if 'junit' in selected:
        count, failed = junit(source, selected['junit'])
        return dict(executed=count, failures=failed)
    text = log.read_text(errors='replace')
    if selected['format'] == 'rust':
        totals = re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed', text)
        failed = rust_failures(text)
        return dict(executed=sum(int(a) + int(b) for a, b in totals), failures=failed)
    count = re.search(r'Ran (\d+) tests? in ', text)
    failed = re.findall(r'^FAIL: (\S+)', text, re.M)
    return dict(executed=int(count[1]) if count else 0, failures=failed)


def rust_failures(text):
    names = set(re.findall(r'^test (\S+) \.\.\. FAILED$', text, re.M))
    for summary in re.findall(r'^failures:\n((?:[ \t]+\S+\n)+)', text, re.M):
        names.update(summary.split())
    return sorted(names)


def classify(build, test, evidence, expected):
    if not passed(build):
        return 'unviable' if build['status'] == 'completed' else build['status']
    if test['status'] != 'completed':
        return test['status']
    if not evidence['executed']:
        return 'tool_error'
    if passed(test):
        return 'survived' if not evidence['failures'] else 'tool_error'
    if any(expected in failure for failure in evidence['failures']):
        return 'caught'
    return 'tool_error'


def clear_reports(source, selected):
    for path in source.glob(selected.get('junit', '__no_mutation_report__')):
        path.unlink()


def phases(source, output, env, selected, limits, label):
    build = invocation(selected['build'], source, output)
    test = invocation(selected['test'], source, output)
    built = phase(build, source, output, env, label + '-build.log', limits.build_timeout, limits.deadline)
    if not passed(built):
        return built, {}, dict(executed=0, failures=[])
    clear_reports(source, selected)
    tested = phase(test, source, output, env, label + '-test.log', limits.test_timeout, limits.deadline)
    evidence = test_evidence(source, selected, output / (label + '-test.log')) if tested['status'] == 'completed' else dict(executed=0, failures=[])
    retain_reports(source, output / (label + '-junit'), selected)
    return built, tested, evidence


def retain_reports(source, output, selected):
    import shutil
    paths = list(source.glob(selected.get('junit', '__no_mutation_report__')))
    if paths:
        output.mkdir()
        for path in paths:
            shutil.copy2(path, output / path.name)


def candidate(row, source, output, env, selected, limits):
    path, original, replacement = edited(source, row)
    diff = difflib.unified_diff(original.splitlines(True), replacement.splitlines(True),
                               fromfile=row['file'], tofile=row['file'])
    (output / (row['id'] + '.diff')).write_text(''.join(diff), encoding='utf-8')
    try:
        path.write_text(replacement, encoding='utf-8', newline='')
        build, test, evidence = phases(source, output, env, selected, limits, row['id'])
        return dict(id=row['id'], outcome=classify(build, test, evidence, row['expected_failure']),
                    build=build, test=test, evidence=evidence)
    finally:
        path.write_text(original, encoding='utf-8', newline='')


def report(selected, rows, baseline, source, hashes):
    build, test, evidence = baseline
    valid = passed(build) and passed(test) and evidence['executed'] > 0 and not evidence['failures']
    intact = unchanged(source, hashes)
    return dict(baseline=dict(build=build, test=test, evidence=evidence),
                outcomes=dict(Counter(row['outcome'] for row in rows)), candidates=rows,
                baseline_passed=valid, snapshot_restored=intact,
                complete=valid and len(rows) == selected['candidate_count'] and intact,
                passes=valid and intact and len(rows) == selected['candidate_count'] and all(row['outcome'] == 'caught' for row in rows),
                whole_project_mutation_score=None)


def campaign(args):
    selected, candidates = catalog(args.catalog, args.profile, sys.platform)
    selected = dict(selected, candidate_count=len(candidates))
    output = args.output.resolve()
    if output.is_relative_to(ROOT.resolve()):
        raise ValueError('Output must be outside the source checkout')
    output.mkdir(parents=True)
    source = output / 'source'
    hashes = snapshot(ROOT, source)
    for row in candidates:
        edited(source, row)
    env = environment(output, args.build_jobs)
    env['CARGO_TARGET_DIR'] = str(output / 'target')
    env['CARGO_INCREMENTAL'] = '0'
    env = isolation.prepare(output, selected, env)
    args.deadline = time.monotonic() + args.campaign_timeout
    manifest = dict(platform=sys.platform, profile=args.profile, sources=hashes,
                    catalog_sha256=fingerprint(args.catalog), selection=selected, inventory=candidates,
                    limits=vars(args) | {'catalog': str(args.catalog), 'output': str(output)})
    write_json(output / 'manifest.json', manifest)
    baseline = phases(source, output, env, selected, args, 'baseline')
    rows = run_candidates(candidates, source, output, env, selected, args, baseline, hashes)
    result = report(selected, rows, baseline, source, hashes)
    result['checkout_matches_snapshot'] = unchanged(ROOT, hashes)
    write_json(output / 'report.json', result)
    print(json.dumps(result, indent=2))
    return int(not result['passes'])


def run_candidates(candidates, source, output, env, selected, limits, baseline, hashes):
    build, test, evidence = baseline
    if not (passed(build) and passed(test) and evidence['executed'] and unchanged(source, hashes)):
        return []
    rows = []
    for row in candidates:
        rows.append(candidate(row, source, output, env, selected, limits))
        write_json(output / 'partial.json', rows)
        if not unchanged(source, hashes):
            break
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalog', type=Path, default=Path(__file__).with_name('catalog.json'))
    parser.add_argument('--profile', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--build-jobs', type=positive, default=4)
    parser.add_argument('--build-timeout', type=positive, default=600)
    parser.add_argument('--test-timeout', type=positive, default=180)
    parser.add_argument('--campaign-timeout', type=positive, default=3600)
    return campaign(parser.parse_args())


if __name__ == '__main__':
    raise SystemExit(main())
