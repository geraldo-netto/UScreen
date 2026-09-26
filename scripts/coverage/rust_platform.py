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


def predicate(nodes, platform="linux"):
    text = ''.join(node.text.decode() for node in nodes)
    known = {'windows': platform == 'windows', 'unix': platform != 'windows'}
    if text in known:
        return known[text]
    if len(nodes) == 3 and nodes[0].text in {b'target_os', b'target_family'}:
        family = 'windows' if platform == 'windows' else 'unix'
        expected = ('"' + (platform if nodes[0].text == b'target_os' else family) + '"').encode()
        return nodes[2].text == expected
    if len(nodes) == 2 and nodes[0].text in {b'all', b'any', b'not'}:
        return combine(nodes[0].text.decode(), [predicate(group, platform) for group in arguments(nodes[1])])
    return None


@cache
def unavailable(text, platform="linux"):
    for node in walk(parse(text, 'rust')):
        if node.type != 'attribute' or node.named_children[0].text != b'cfg':
            continue
        tree = node.named_children[-1]
        if predicate(tree.children[1:-1], platform) is False:
            return True
    return False


def guarded(node, platform="linux"):
    while node is not None:
        if unavailable(attributes(node), platform):
            return True
        node = node.parent
    return False
