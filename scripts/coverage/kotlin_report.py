"""Use method counters for Kotlin callbacks; creating a lambda never covers its body."""
import xml.etree.ElementTree as ET

from model import add_line, result, source_path, validate_count
from readers import read_bounded


def jvm_names(name):
    if ':' not in name:
        return {name}
    kind, property_name = name.split(':', 1)
    title = property_name[0].upper() + property_name[1:]
    if kind == 'get':
        return {'get' + title, property_name}
    return {'set' + title, 'set' + property_name.removeprefix('is')}


def matches(function, method):
    if not (function.body or function.first) <= method['line'] <= function.last:
        return False
    name = method['name']
    if name.endswith('$default') or name.startswith('access$'):
        return False
    if function.name == '<lambda>':
        return name in {'invoke', 'invokeSuspend'} or '$lambda$' in name
    if '$lambda$' in name:
        return False
    return name.split('$')[0] in jvm_names(function.name)


def function_key(function):
    return function.file, function.first, function.last, function.name


def assign_methods(functions, methods):
    assignments = {}
    by_file = {}
    for function in functions:
        by_file.setdefault(function.file, []).append(function)
    for method in methods:
        candidates = [function for function in by_file.get(method['file'], []) if matches(function, method)]
        if candidates:
            owner = min(candidates, key=lambda function: function.last - function.first)
            assignments.setdefault(function_key(owner), []).append(method)
    return assignments


def method_record(file, owner, method):
    counter = method.find("counter[@type='LINE']")
    if counter is None or 'line' not in method.attrib:
        return None
    covered = validate_count(int(counter.attrib['covered']))
    missed = validate_count(int(counter.attrib['missed']))
    return dict(file=file, owner=owner, name=method.attrib['name'], signature=method.attrib['desc'],
                line=int(method.attrib['line']), covered=covered, total=covered + missed)


def read_methods(package, directory, root):
    output = []
    for owner in package.findall('class'):
        file = source_path(root, str(directory / owner.attrib.get('sourcefilename', '')))
        if file is None:
            continue
        for method in owner.findall('method'):
            record = method_record(file, owner.attrib['name'], method)
            if record is not None:
                output.append(record)
    return output


def strict_lines(package, directory, root, data):
    for source in package.findall('sourcefile'):
        file = source_path(root, str(directory / source.attrib['name']))
        if file is None:
            continue
        for line in source.findall('line'):
            covered = validate_count(int(line.attrib['ci']))
            missed = validate_count(int(line.attrib['mi']))
            if covered + missed:
                add_line(data, file, int(line.attrib['nr']), int(covered > 0 and missed == 0))


def read(path, source_root, root):
    report = ET.fromstring(read_bounded(path))
    methods, data = [], {}
    for package in report.findall('package'):
        directory = source_root / package.attrib['name']
        methods.extend(read_methods(package, directory, root))
        strict_lines(package, directory, root, data)
    return methods, data


def function_result(function, methods, data):
    selected = [method for method in methods if method['file'] == function.file and matches(function, method)]
    row = result(function, data)
    row['evidence'] = 'source lines with all instructions covered (conservative fallback)'
    if not selected:
        return row
    worst = min(selected, key=lambda method: method['covered'] / max(method['total'], 1))
    covered, total = worst['covered'], worst['total']
    row.update(covered=covered, total=total, percent=100 * covered / total if total else None,
               passes=bool(total) and covered * 5 >= total * 4, missing=[],
               evidence='JaCoCo method line counters; every mapped variant must pass', variants=selected)
    return row
