"""T479 isolated USB replay framing: bounded stock tee packets and ACKs."""
import struct
import threading
import time
import zlib


class TeePackets:
    def __init__(self):
        self.pending = bytearray()
        self.packet = None
        self.headers = []
        self.previous = None

    def feed(self, data):
        self.pending.extend(data)
        output = []
        while True:
            if self.packet is None and not self.header():
                return output
            if self.packet is None:
                continue
            size, checksum, pts = self.packet
            if len(self.pending) < size:
                return output
            payload = bytes(self.pending[:size])
            del self.pending[:size]
            if zlib.adler32(payload, 0) != checksum:
                raise ValueError('stock tee packet checksum mismatch')
            output.append((payload, pts))
            self.packet = None

    def header(self):
        end = self.pending.find(b'\n')
        if end < 0:
            if len(self.pending) > 512:
                raise ValueError('oversized tee line')
            return False
        if end > 512:
            raise ValueError('oversized tee line')
        line = bytes(self.pending[:end]).decode('ascii')
        del self.pending[:end + 1]
        if line.startswith('#'):
            self.headers.append(line)
            if len(self.headers) > 32:
                raise ValueError('too many tee headers')
        else:
            self.packet = self.fields(line)
        return True

    def fields(self, line):
        cells = [cell.strip() for cell in line.split(',')]
        if len(cells) < 6 or int(cells[0]) != 0:
            raise ValueError('invalid tee packet metadata')
        dts, pts, size = int(cells[1]), int(cells[2]), int(cells[4])
        if not 1 <= size <= 8 * 1024 * 1024 or (self.previous is not None and dts <= self.previous):
            raise ValueError('invalid tee packet size/order')
        self.previous = dts
        return size, int(cells[5], 16), pts

    def finish(self):
        if self.pending or self.packet is not None:
            raise ValueError('truncated tee packet')


def exact(stream, count):
    data = bytearray()
    while len(data) < count:
        block = stream.read(count - len(data))
        if not block:
            raise EOFError('truncated replay response')
        data.extend(block)
    return bytes(data)


class Acknowledgements:
    def __init__(self, connection, receipt):
        self.connection, self.receipt = connection, receipt
        self.rows, self.ready, self.failure = [], [], None
        self.stopping = False
        self.closed = False
        self.thread = threading.Thread(target=self.run, daemon=True, name='profile-usb-acks')

    def read(self, stream):
        kind = exact(stream, 1)[0]
        now = time.monotonic_ns()
        if kind == 0:
            size = struct.unpack('!H', exact(stream, 2))[0]
            if size > 512:
                raise ValueError('oversized decoder receipt')
            receipt = exact(stream, size).decode('ascii')
            setup_us = struct.unpack('!Q', exact(stream, 8))[0]
            if receipt != self.receipt:
                raise ValueError('configured decoder receipt differs from request')
            self.ready.append(dict(received_ns=now, setup_us=setup_us, receipt=receipt))
        elif kind == 1:
            sequence, decode_us = struct.unpack('!Ii', exact(stream, 8))
            self.rows.append(dict(sequence=sequence, decode_us=decode_us, acknowledged_ns=time.monotonic_ns()))
        else:
            raise ValueError('unknown replay response')

    def run(self):
        try:
            with self.connection.makefile('rb', buffering=0) as stream:
                while not self.stopping:
                    self.read(stream)
        except EOFError:
            self.closed = True
        except OSError as error:
            if not self.stopping:
                self.failure = str(error)
        except Exception as error:
            self.failure = str(error)

    def wait_ready(self, count):
        deadline = time.monotonic() + 5
        while len(self.ready) < count:
            if self.failure or time.monotonic() >= deadline:
                raise RuntimeError(self.failure or 'decoder setup deadline')
            time.sleep(.01)


def receipt(choice):
    stream = choice['stream']
    return ':'.join(map(str, [len(choice['name']), choice['name'], stream['codec'], stream['profile'],
                            stream['level'], stream['depth'], int(choice['low_latency']), choice['operating_rate'] or 0]))
