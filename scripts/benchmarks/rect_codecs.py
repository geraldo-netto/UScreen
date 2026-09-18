"""T419 fixture-only native compression bindings; no live host capture changes."""
import ctypes as C
import ctypes.util


class RectCodecs:
    def __init__(self):
        self.lz4 = C.CDLL(ctypes.util.find_library('lz4'))
        self.zstd = C.CDLL(ctypes.util.find_library('zstd'))
        self.bind(self.lz4, 'LZ4_compressBound', [C.c_int], C.c_int)
        signature = [C.c_void_p, C.c_void_p, C.c_int, C.c_int]
        self.bind(self.lz4, 'LZ4_compress_default', signature, C.c_int)
        self.bind(self.lz4, 'LZ4_decompress_safe', signature, C.c_int)
        self.bind(self.lz4, 'LZ4_versionString', [], C.c_char_p)
        self.bind(self.zstd, 'ZSTD_compressBound', [C.c_size_t], C.c_size_t)
        signature = [C.c_void_p, C.c_size_t, C.c_void_p, C.c_size_t]
        self.bind(self.zstd, 'ZSTD_compress', signature + [C.c_int], C.c_size_t)
        self.bind(self.zstd, 'ZSTD_decompress', signature, C.c_size_t)
        self.bind(self.zstd, 'ZSTD_isError', [C.c_size_t], C.c_uint)
        self.bind(self.zstd, 'ZSTD_versionString', [], C.c_char_p)

    @staticmethod
    def bind(library, name, arguments, result):
        function = getattr(library, name)
        function.argtypes, function.restype = arguments, result

    def encode(self, codec, raw):
        bound = self.lz4.LZ4_compressBound(len(raw)) if codec == 1 else self.zstd.ZSTD_compressBound(len(raw))
        output = C.create_string_buffer(bound)
        if codec == 1:
            size = self.lz4.LZ4_compress_default(raw, output, len(raw), bound)
            assert size > 0
        else:
            size = self.zstd.ZSTD_compress(output, bound, raw, len(raw), 1)
            assert not self.zstd.ZSTD_isError(size)
        encoded = output.raw[:size]
        assert self.decode(codec, encoded, len(raw)) == raw
        return encoded

    def decode(self, codec, packed, expected):
        output = C.create_string_buffer(expected)
        if codec == 1:
            size = self.lz4.LZ4_decompress_safe(packed, output, len(packed), expected)
        else:
            size = self.zstd.ZSTD_decompress(output, expected, packed, len(packed))
        assert size == expected
        return output.raw

    def versions(self):
        return dict(lz4=self.lz4.LZ4_versionString().decode(), zstd=self.zstd.ZSTD_versionString().decode())
