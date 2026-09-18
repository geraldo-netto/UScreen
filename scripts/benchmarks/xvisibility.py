"""Read-only X11 geometry/coverage checks, plus one deliberate startup focus."""
import re

from Xlib import X, display


def overlaps(a, b):
    x, y, width, height = a
    other_x, other_y, other_width, other_height = b
    return x < other_x + other_width and other_x < x + width and y < other_y + other_height and other_y < y + height


def contains(outer, inner):
    x, y, width, height = outer
    ix, iy, iw, ih = inner
    return x <= ix and y <= iy and ix + iw <= x + width and iy + ih <= y + height


class XVisibility:
    def __init__(self, window_id, geometry, display_name=None):
        self.display = display.Display(display_name)
        try:
            self.window = self.display.create_resource_object('window', window_id)
            self.geometry = tuple(map(int, re.fullmatch(r'(\d+)x(\d+)\+(\d+)\+(\d+)', geometry).groups()))
        except BaseException:
            self.close()
            raise

    def close(self):
        self.display.close()

    def focus(self):
        self.window.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.display.sync()

    def hierarchy(self, window):
        result = []
        for _ in range(64):
            if not hasattr(window, 'id'):
                return result
            if window.id == self.display.screen().root.id:
                return result
            result.append(window)
            window = window.query_tree().parent
        raise ValueError('window ancestry exceeds 64 levels')

    def focused(self, hierarchy):
        focus = self.hierarchy(self.display.get_input_focus().focus)
        return bool(focus and focus[-1].id == hierarchy[-1].id)

    def bounds(self, window, border=False):
        actual = window.get_geometry()
        position = self.display.screen().root.translate_coords(window, 0, 0)
        margin = actual.border_width if border else 0
        return (position.x - margin, position.y - margin,
                actual.width + 2 * margin, actual.height + 2 * margin)

    def obscured(self, window, target):
        tree = window.query_tree()
        if not contains(self.bounds(tree.parent), target):
            return True
        siblings = tree.parent.query_tree().children
        index = next(i for i, sibling in enumerate(siblings) if sibling.id == window.id)
        for sibling in siblings[index + 1:]:
            attrs = sibling.get_attributes()
            if attrs.map_state == X.IsViewable and attrs.win_class == X.InputOutput:
                if overlaps(self.bounds(sibling, border=True), target):
                    return True
        return False

    def problem(self):
        width, height, x, y = self.geometry
        target = self.bounds(self.window)
        if target != (x, y, width, height):
            return 'workload geometry changed'
        if self.window.get_attributes().map_state != X.IsViewable:
            return 'workload is not viewable'
        hierarchy = self.hierarchy(self.window)
        if not self.focused(hierarchy):
            return 'workload lost keyboard focus'
        if any(self.obscured(window, target) for window in hierarchy):
            return 'workload is partially or fully occluded'
        return None
