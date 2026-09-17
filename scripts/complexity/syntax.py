"""Source metrics, deliberately independent of Sonar's licensed implementation."""
import ast
from tree_sitter import Language, Parser
import tree_sitter_bash
import tree_sitter_c
import tree_sitter_rust

PARSERS = {name: Parser(Language(module.language())) for name, module in
           [('rust', tree_sitter_rust), ('c', tree_sitter_c), ('shell', tree_sitter_bash)]}


def walk(node):
    if node.is_extra:
        return
    yield node
    for child in node.children:
        yield from walk(child)


def parse(source, language):
    tree = PARSERS[language].parse(source.encode())
    if tree.root_node.has_error:
        raise ValueError(f'{language} parse error: audit cannot certify this file')
    return tree.root_node


def nonempty(node, field):
    value = node.child_by_field_name(field)
    return value is not None and (value.type != 'block' or value.named_child_count > 0)


def logical(node):
    return node.type == 'binary_expression' and any(c.type in {'&&', '||'} for c in node.children)


def rust_increment(node):
    simple = {'if_expression', 'loop_expression', 'while_expression',
              'for_expression', 'closure_expression'}
    fields = {'function_item': 'body', 'match_arm': 'value'}
    if node.type in fields:
        return int(nonempty(node, fields[node.type]))
    return int(node.type in simple or logical(node))


def c_increment(node):
    branches = {'if_statement', 'for_statement', 'while_statement', 'do_statement',
                'case_statement', 'conditional_expression'}
    return int(node.type in branches or logical(node))


def shell_increment(node):
    return int(node.type in {'if_statement', 'elif_clause', 'for_statement',
                            'c_style_for_statement', 'while_statement',
                            'case_item', '&&', '||'})


def function_name(node):
    name = node.child_by_field_name('name')
    if name is not None:
        return name.text.decode()
    current = node.child_by_field_name('declarator')
    while current is not None and current.type != 'identifier':
        current = current.child_by_field_name('declarator')
    return current.text.decode() if current is not None else '<anonymous>'


def syntax_scores(source, language):
    kind = 'function_item' if language == 'rust' else 'function_definition'
    increment = {'rust': rust_increment, 'c': c_increment, 'shell': shell_increment}[language]
    base = 0 if language == 'rust' else 1
    for node in walk(parse(source, language)):
        if node.type == kind:
            yield node.start_point.row + 1, function_name(node), base + sum(map(increment, walk(node)))


class PythonScore(ast.NodeVisitor):
    def __init__(self, source):
        self.score = 0
        self.level = 0
        self.source = source.splitlines()

    def visit_FunctionDef(self, node):
        if self.level:
            return
        self.level += 1
        self.branch(node)
        self.level -= 1

    visit_AsyncFunctionDef = visit_FunctionDef

    def visit_If(self, node):
        if not self.source[node.lineno - 1].lstrip().startswith('elif '):
            self.score += 1
        self.generic_visit(node)

    def branch(self, node):
        self.score += 1
        self.generic_visit(node)

    visit_For = branch
    visit_AsyncFor = branch
    visit_While = branch
    visit_IfExp = branch

    def visit_BoolOp(self, node):
        self.score += len(node.values) - 1
        self.generic_visit(node)

    def visit_comprehension(self, node):
        self.score += len(node.ifs)
        self.generic_visit(node)


def python_scores(source):
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            visitor = PythonScore(source)
            visitor.visit(node)
            yield node.lineno, node.name, visitor.score


def embedded_python(source):
    for node in walk(parse(source, 'shell')):
        if node.type != 'heredoc_redirect':
            continue
        command = node.parent.child_by_field_name('body')
        if command is None:
            continue
        name = command.child_by_field_name('name')
        if name is not None and name.text in {b'python', b'python3'}:
            body = next(child for child in node.children if child.type == 'heredoc_body')
            yield body.start_point.row, body.text.decode()
