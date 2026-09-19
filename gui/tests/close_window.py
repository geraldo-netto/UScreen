"""T497: close only the owned GUI window on the isolated Xvfb server."""
import sys
from Xlib import X, display, protocol

connection = display.Display()
pid = int(sys.argv[1])
pid_atom = connection.intern_atom('_NET_WM_PID')
targets = [window for window in connection.screen().root.query_tree().children
           if (value := window.get_full_property(pid_atom, X.AnyPropertyType)) is not None
           and list(value.value) == [pid]]
if len(targets) != 1:
    raise RuntimeError(f'T497 expected one owned window, found {len(targets)}')
target = targets[0]
target.send_event(protocol.event.ClientMessage(window=target,
    client_type=connection.intern_atom('WM_PROTOCOLS'),
    data=(32, [connection.intern_atom('WM_DELETE_WINDOW'), X.CurrentTime, 0, 0, 0])))
connection.sync()
connection.close()
