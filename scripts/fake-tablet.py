#!/usr/bin/env python3
"""Pretend to be a tablet on the loopback ports.

Connects to the daemon's input WebSocket and video stream for one slot,
authenticates with the session token, reports a resolution, and acks every
frame it receives - enough to drive a whole pipeline without a device.
Used together with USCREEN_FAKE_TABLET=<serial> and max_tablets > 1 to test
a second slot with a single physical tablet.

    scripts/fake-tablet.py --slot 1 --seconds 20
"""
import argparse, base64, json, os, socket, struct, sys, time, threading

def runtime_dir():
    base = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return os.path.join(base, "uscreen")

class BufferedSocket:
    """Keep bytes received after the HTTP upgrade for the first WS frame."""
    def __init__(self, sock, pending):
        self.sock, self.pending = sock, pending

    def recv(self, size):
        if self.pending:
            result, self.pending = self.pending[:size], self.pending[size:]
            return result
        return self.sock.recv(size)

    def __getattr__(self, name):
        return getattr(self.sock, name)


def ws_connect(port):
    s = socket.create_connection(("127.0.0.1", port), timeout=5)
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall((f"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
            f"Sec-WebSocket-Version: 13\r\n\r\n").encode())
    resp = b""
    while b"\r\n\r\n" not in resp:
        chunk = s.recv(4096)
        if not chunk:
            raise RuntimeError("handshake: connection closed")
        resp += chunk
    if b" 101 " not in resp.split(b"\r\n")[0]:
        raise RuntimeError("handshake failed: " + resp.split(b"\r\n")[0].decode())
    return BufferedSocket(s, resp.split(b"\r\n\r\n", 1)[1])

def ws_send(s, obj):
    payload = json.dumps(obj).encode()
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    if len(payload) < 126:
        hdr = bytes([0x81, 0x80 | len(payload)])
    else:
        hdr = bytes([0x81, 0x80 | 126]) + struct.pack(">H", len(payload))
    s.sendall(hdr + mask + masked)

def ws_recv_text(s):
    try:
        h = read_exact(s, 2)
        op, ln = h[0] & 0x0F, h[1] & 0x7F
        if ln == 126:
            ln = struct.unpack(">H", read_exact(s, 2))[0]
        elif ln == 127:
            ln = struct.unpack(">Q", read_exact(s, 8))[0]
        data = read_exact(s, ln)
        return data.decode(errors="replace") if op == 1 else None
    except EOFError:
        return None

def read_exact(s, n):
    buf = b""
    while len(buf) < n:
        part = s.recv(n - len(buf))
        if not part:
            raise EOFError
        buf += part
    return buf

def receive_video(video, control, seconds):
    frames = 0
    got_config = False
    t_end = time.time() + seconds
    seq_last = None
    while time.time() < t_end:
        try:
            ln = struct.unpack(">I", read_exact(video, 4))[0]
            body = read_exact(video, ln)
        except (socket.timeout, EOFError):
            break
        ptype = body[0]
        if ptype == 0:
            got_config = True
            print(f"codec config: {ln-1} bytes")
        elif ptype == 1:
            seq = struct.unpack(">I", body[1:5])[0]
            frames += 1
            seq_last = seq
            ws_send(control, {"type": "rendered", "seq": seq, "decode_us": 1000})
    return got_config, frames, seq_last


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--slot", type=int, default=1)
    ap.add_argument("--seconds", type=int, default=20)
    ap.add_argument("--width", type=int, default=2560)
    ap.add_argument("--height", type=int, default=1600)
    ap.add_argument("--no-token", action="store_true", help="skip auth, to check we get dropped")
    a = ap.parse_args()
    vport, iport = 8890 + 2 * a.slot, 8891 + 2 * a.slot
    token = None
    if not a.no_token:
        with open(os.path.join(runtime_dir(), "token")) as f:
            token = f.read().strip()

    # --- control channel ---
    ws = ws_connect(iport)
    if token:
        ws_send(ws, {"type": "auth", "token": token})
    ws.settimeout(5)
    greeting = ws_recv_text(ws)
    print("greeting:", greeting)
    if not greeting:
        print("dropped before greeting (expected without a token)")
        return 0 if a.no_token else 1
    ws_send(ws, {"type": "resolution", "width": a.width, "height": a.height,
                 "width_mm": 300, "height_mm": 190})
    print(f"reported {a.width}x{a.height}")

    # --- video ---
    v = socket.create_connection(("127.0.0.1", vport), timeout=10)
    if token:
        v.sendall(token.encode())
    got_config, frames, seq_last = receive_video(v, ws, a.seconds)
    print(f"frames: {frames}, config: {got_config}, last seq: {seq_last}")
    ok = got_config and frames > 0
    print("RESULT:", "OK" if ok else "FAIL")
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
