"""T608 research-only authenticated fragments; NOT RTP, SRTP or production wire."""
import hashlib
import hmac
import math
import socket
import struct
import time

HEADER = struct.Struct('!IHHI')
PAYLOAD = 1152
DEADLINE = .150
WINDOW = 8


def fragments(key, sequence, packet):
    count = math.ceil(len(packet) / PAYLOAD)
    for index in range(count):
        body = HEADER.pack(sequence, index, count, len(packet)) + packet[index*PAYLOAD:(index+1)*PAYLOAD]
        yield body + hmac.digest(key, body, hashlib.sha256)[:16]


def authenticated(key, data, frames):
    if len(data) < HEADER.size + 17:
        return None
    body, tag = data[:-16], data[-16:]
    if not hmac.compare_digest(hmac.digest(key, body, hashlib.sha256)[:16], tag):
        return None
    sequence, index, count, size = HEADER.unpack_from(body)
    if not (sequence < frames and 0 < size <= 2*1024*1024):
        return None
    if count != math.ceil(size / PAYLOAD) or not index < count:
        return None
    payload = body[HEADER.size:]
    expected = min(PAYLOAD, size - index * PAYLOAD)
    return (sequence, index, count, size, payload) if len(payload) == expected else None


class Receiver:
    def __init__(self, packets, keys, key, start):
        self.packets, self.keys, self.key, self.start = packets, keys, key, start
        self.pending, self.complete, self.accepted = {}, [], []
        self.expected, self.invalid, self.peak, self.expired = 0, 0, 0, 0
        self.need_key = False

    def insert(self, data):
        parsed = authenticated(self.key, data, len(self.packets))
        if parsed is None:
            self.invalid += 1
            return
        sequence, index, count, size, payload = parsed
        if not self.expected <= sequence < self.expected + WINDOW:
            return
        entry = self.pending.setdefault(sequence, (count, size, {}))
        if entry[:2] != (count, size):
            self.invalid += 1
            return
        entry[2][index] = payload
        self.peak = max(self.peak, sum(len(v) for row in self.pending.values() for v in row[2].values()))

    def deliver(self, sequence, entry):
        data = b''.join(entry[2][i] for i in range(entry[0]))
        assert data == self.packets[sequence], 'byte identity failed'
        age = (time.monotonic() - self.start - sequence/30) * 1000
        row = dict(sequence=sequence, age_ms=age)
        self.complete.append(row)
        if not self.need_key or sequence in self.keys:
            self.accepted.append(row)
            self.need_key = False

    def drain(self):
        while self.expected < len(self.packets):
            entry = self.pending.get(self.expected)
            ready = entry is not None and len(entry[2]) == entry[0]
            if time.monotonic() > self.start + self.expected/30 + DEADLINE:
                self.expired += 1
                self.need_key = True
            elif ready:
                self.deliver(self.expected, entry)
            else:
                return
            self.pending.pop(self.expected, None)
            self.expected += 1

    def receive(self, receiver):
        receiver.settimeout(.01)
        while self.expected < len(self.packets):
            try:
                self.insert(receiver.recv(1500))
            except socket.timeout:
                pass
            self.drain()
        return dict(arrivals=self.accepted, complete=self.complete, expired=self.expired,
                    invalid=self.invalid, peak_reassembly_bytes=self.peak)
