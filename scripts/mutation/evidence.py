"""Strict cargo-mutants 27.1.0 result import; incomplete runs never pass."""
from collections import Counter
import json
import re

CLASSES = {'CaughtMutant': 'caught', 'MissedMutant': 'survived',
           'Unviable': 'unviable', 'Timeout': 'timeout'}


def identity(mutant):
    return json.dumps([mutant['file'], mutant['span'], mutant['replacement']], sort_keys=True)


def baseline_passed(outcomes, directory):
    baselines = [row for row in outcomes if row['scenario'] == 'Baseline']
    if len(baselines) != 1:
        return False
    row = baselines[0]
    phases = {part['phase']: part['process_status'] for part in row['phase_results']}
    log = (directory / row['log_path']).read_text(errors='replace')
    return (row['summary'] == 'Success' and phases == {'Build': 'Success', 'Test': 'Success'}
            and any(int(n) for n in re.findall(r'test result: ok\. (\d+) passed', log)))


def classify(row):
    value = CLASSES.get(row['summary'], 'tool_error')
    phases = {part['phase']: part['process_status'] for part in row['phase_results']}
    if value in {'caught', 'survived'} and phases.get('Build') != 'Success':
        return 'tool_error'
    if value == 'caught' and not failed_test(phases.get('Test')):
        return 'tool_error'
    if value == 'survived' and phases.get('Test') != 'Success':
        return 'tool_error'
    return value


def failed_test(status):
    return (isinstance(status, dict) and isinstance(status.get('Failure'), int)
            and status['Failure'] != 0)


def command_signature(argv):
    if not argv:
        return None
    packages = set()
    arguments = []
    for argument in argv:
        if argument.startswith('--package='):
            packages.add(argument.split('=', 1)[1].split('@', 1)[0])
        else:
            arguments.append(argument)
    return arguments, sorted(packages)


def phase_commands(row):
    return {part['phase']: command_signature(part.get('argv'))
            for part in row['phase_results']}


def baseline_matches(outcomes):
    baselines = [row for row in outcomes if row['scenario'] == 'Baseline']
    if len(baselines) != 1:
        return False
    expected = phase_commands(baselines[0])
    if set(expected) != {'Build', 'Test'} or not all(expected.values()):
        return False
    return all(commands_match(row, expected) for row in outcomes)


def commands_match(row, expected):
    actual = phase_commands(row)
    return bool(actual) and all(command is not None and command == expected.get(phase)
                                for phase, command in actual.items())


def inventory_match(candidates, rows):
    expected = [identity(row) for row in candidates]
    actual = [identity(row['scenario']['Mutant']) for row in rows]
    unique = len(set(expected)) == len(expected) and len(set(actual)) == len(actual)
    return unique, bool(expected) and unique and set(actual) == set(expected)


def summarize(directory, process):
    candidates = json.loads((directory / 'mutants.json').read_text())
    outcomes = json.loads((directory / 'outcomes.json').read_text())['outcomes']
    rows = [row for row in outcomes if row['scenario'] != 'Baseline']
    unique, complete = inventory_match(candidates, rows)
    counts = Counter(classify(row) for row in rows)
    baseline = baseline_passed(outcomes, directory)
    matching = baseline_matches(outcomes)
    valid_process = process['status'] == 'completed' and process.get('code') in {0, 2, 3, 4}
    passed = baseline and matching and complete and valid_process and counts['caught'] == len(candidates)
    return dict(baseline_passed=baseline, baseline_matches_mutants=matching,
                complete=complete, unique=unique,
                candidates=len(candidates), outcomes=dict(counts), passes=passed,
                whole_project_mutation_score=None)
