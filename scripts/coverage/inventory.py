"""Owned source functions; test-only code is excluded by location or Rust cfg."""
import ast
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'complexity'))
from check import source_files, language, rpm_sections
from syntax import parse, walk, function_name, embedded_python
from model import Function


def fixture(path):
    return bool({'test', 'tests', 'testdata', 'test_support'} & set(path.parts)
                or re.search(r'(^test_|^tests?\.|_tests?\.|_test_support\.)', path.name))


def essential_script(path):
    if path.parts[0] == 'packaging':
        return language(path) in {'python', 'shell', 'rpm'} or path.name == 'AppRun'
    return str(path) in {
        'scripts/build-release.sh', 'scripts/copy-distribution-docs.sh', 'scripts/gen-edid.py',
        'scripts/install.sh', 'scripts/setup-evdi.sh', 'scripts/stage-linux-bundle.sh',
        'scripts/verify-release-apk.py', 'scripts/write-desktop-entry.sh', 'scripts/write-systemd-service.sh',
        'scripts/ci/build-artifacts.sh', 'scripts/ci/build-arch-package.sh',
        'scripts/ci/verify-portability.py',
    }


def coverage_source(path):
    if essential_script(path):
        return True
    if {'benchmarks', 'benches', 'examples', 'docs', 'scripts'} & set(path.parts):
        return False
    if path.suffix == '.rs':
        return True
    if path.suffix == '.c':
        return path.as_posix().startswith('host/evdi/')
    return path.suffix == '.kt' and path.as_posix().startswith('android/app/src/')


def attributes(node):
    sibling = node.prev_named_sibling
    result = []
    while sibling is not None and sibling.type in {'attribute_item', 'line_comment', 'block_comment'}:
        result.append(sibling.text.decode())
        sibling = sibling.prev_named_sibling
    return '\n'.join(result)


def test_only(node):
    while node is not None:
        if re.search(r'#\[(?:tokio::)?test\b|#\[cfg\(\s*(?:all\(\s*)?test\b', attributes(node)):
            return True
        node = node.parent
    return False


def module_path(root, path, node):
    match = re.search(r'#\[path\s*=\s*"([^"]+)"\]', attributes(node))
    if match is not None:
        return (root / path.parent / match[1]).resolve()
    if node.child_by_field_name('body') is not None:
        return None
    base = path.parent if path.stem in {'lib', 'main', 'mod', 'linux_main', 'windows_main'} else path.with_suffix('')
    name = node.child_by_field_name('name').text.decode()
    for relative in [base / (name + '.rs'), base / name / 'mod.rs']:
        if (root / relative).is_file():
            return (root / relative).resolve()
    return None


def rust_test_files(root, paths):
    references = {}
    for path in paths:
        for node in walk(parse((root / path).read_text(), 'rust')):
            if node.type != 'mod_item':
                continue
            destination = module_path(root, path, node)
            if destination is not None:
                references.setdefault(destination, []).append(((root / path).resolve(), test_only(node)))
    return test_dependencies(root, paths, references)


def test_dependencies(root, paths, references):
    excluded = {(root / path).resolve() for path in paths if fixture(path)}
    while True:
        found = {path for path, owners in references.items()
                 if all(test or source in excluded for source, test in owners)}
        if found <= excluded:
            return excluded
        excluded.update(found)


def syntax_functions(path, source, kind):
    node_type = 'function_item' if kind == 'rust' else 'function_definition'
    nodes = [node for node in walk(parse(source, kind)) if node.type == node_type]
    for node in nodes:
        if kind == 'rust' and test_only(node):
            continue
        nested = tuple((child.start_point.row + 1, child.end_point.row + 1)
                       for child in nodes if child != node
                       and node.start_byte < child.start_byte < node.end_byte)
        yield Function(str(path), function_name(node), node.start_point.row + 1,
                       node.end_point.row + 1, kind, nested)


def python_functions(path, source):
    kinds = (ast.FunctionDef, ast.AsyncFunctionDef)
    nodes = [node for node in ast.walk(ast.parse(source)) if isinstance(node, kinds)]
    for node in nodes:
        nested = tuple((child.lineno, child.end_lineno) for child in nodes
                       if node.lineno < child.lineno <= node.end_lineno)
        entry = min([node.lineno] + [item.lineno for item in node.decorator_list])
        yield Function(str(path), node.name, node.lineno, node.end_lineno, 'python', nested,
                       entry=entry, body=node.body[0].lineno)


def file_functions(path, source, kind):
    if kind == 'python':
        yield from python_functions(path, source)
    elif kind in {'rust', 'c', 'shell'}:
        yield from syntax_functions(path, source, kind)
    if kind == 'shell':
        for offset, body in embedded_python(source):
            yield from python_functions(path, '\n' * offset + body)
    elif kind == 'rpm':
        for offset, body in rpm_sections(source):
            yield from file_functions(path, '\n' * offset + body, 'shell')


def owned_functions(root):
    all_paths = source_files(root)
    paths = [path for path in all_paths if not fixture(path)]
    app_run = Path('packaging/appimage/AppRun')
    if (root / app_run).is_file():
        paths.append(app_run)
    test_files = rust_test_files(root, [path for path in all_paths if path.suffix == '.rs'])
    for path in paths:
        if (root / path).resolve() in test_files:
            continue
        source = (root / path).read_text()
        kind = 'shell' if path == app_run else language(path)
        yield from file_functions(path, source, kind)
