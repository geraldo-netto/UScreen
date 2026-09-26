"""T591: exclude only Rust cfg predicates provably unavailable on Linux.

Unknown feature/target predicates remain in scope. None means either truth value
is possible; retaining such code prevents missing counters from becoming a pass.
"""
from functools import cache

from inventory import attributes, parse, walk


def arguments(tree):
    groups = [[]]
    for child in tree.children[1:-1]:
        if child.type == ',':
            groups.append([])
        else:
            groups[-1].append(child)
    return [group for group in groups if group]


def combine(operator, values):
    if operator == 'not':
        return None if len(values) != 1 or values[0] is None else not values[0]
    decisive = operator == 'any'
    if decisive in values:
        return decisive
    return None if None in values else not decisive


def predicate(nodes):
    text = ''.join(node.text.decode() for node in nodes)
    known = {'windows': False, 'unix': True}
    if text in known:
        return known[text]
    if len(nodes) == 3 and nodes[0].text in {b'target_os', b'target_family'}:
        expected = b'"linux"' if nodes[0].text == b'target_os' else b'"unix"'
        return nodes[2].text == expected
    if len(nodes) == 2 and nodes[0].text in {b'all', b'any', b'not'}:
        return combine(nodes[0].text.decode(), [predicate(group) for group in arguments(nodes[1])])
    return None


@cache
def unavailable(text):
    for node in walk(parse(text, 'rust')):
        if node.type != 'attribute' or node.named_children[0].text != b'cfg':
            continue
        tree = node.named_children[-1]
        if predicate(tree.children[1:-1]) is False:
            return True
    return False


def guarded(node):
    while node is not None:
        if unavailable(attributes(node)):
            return True
        node = node.parent
    return False
