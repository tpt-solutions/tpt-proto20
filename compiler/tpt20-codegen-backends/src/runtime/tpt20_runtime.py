"""tpt20 minimal Python runtime: native wire format, decoder limits, errors.

Generated together with the message code; do not edit. Implements the same
format and the same limits as the Rust runtime (spec sections 9 and 18).
"""
import struct


class DecodeError(Exception):
    """Malformed or limit-violating input."""


class Limits:
    """Bounds enforced while decoding untrusted input (Rust `DecoderLimits`)."""

    def __init__(
        self,
        max_message_bytes=4 * 1024 * 1024,
        max_depth=100,
        max_field_count=32 * 1024,
        max_unknown_field_bytes=4 * 1024 * 1024,
        max_string_bytes=4 * 1024 * 1024,
        max_bytes_field_bytes=16 * 1024 * 1024,
        max_repeated_entries=512 * 1024,
        max_map_entries=512 * 1024,
    ):
        self.max_message_bytes = max_message_bytes
        self.max_depth = max_depth
        self.max_field_count = max_field_count
        self.max_unknown_field_bytes = max_unknown_field_bytes
        self.max_string_bytes = max_string_bytes
        self.max_bytes_field_bytes = max_bytes_field_bytes
        self.max_repeated_entries = max_repeated_entries
        self.max_map_entries = max_map_entries


DEFAULT_LIMITS = Limits()

# Wire classes.
VARINT, FIXED32, FIXED64, LEN = 0, 1, 2, 3

_M64 = 0xFFFFFFFFFFFFFFFF

_CLASS = {
    "bool": VARINT, "int32": VARINT, "int64": VARINT, "uint32": VARINT,
    "uint64": VARINT, "sint32": VARINT, "sint64": VARINT,
    "fixed32": FIXED32, "sfixed32": FIXED32, "float32": FIXED32,
    "fixed64": FIXED64, "sfixed64": FIXED64, "float64": FIXED64,
    "string": LEN, "bytes": LEN,
}


def scalar_class(name):
    return _CLASS[name]


# ---- varints ---------------------------------------------------------------

def write_varint(out, v):
    v &= _M64
    while v >= 0x80:
        out.append((v & 0x7F) | 0x80)
        v >>= 7
    out.append(v)


def read_varint(buf, pos):
    result = 0
    shift = 0
    n = len(buf)
    for _ in range(10):
        if pos >= n:
            raise DecodeError("unexpected end of input (truncated message)")
        b = buf[pos]
        pos += 1
        result |= (b & 0x7F) << shift
        if not b & 0x80:
            if result > _M64:
                raise DecodeError("varint too long (would overflow 64 bits)")
            return result, pos
        shift += 7
    raise DecodeError("varint too long (would overflow 64 bits)")


def zigzag_encode(n):
    return ((n << 1) ^ (n >> 63)) & _M64


def zigzag_decode(u):
    return (u >> 1) ^ -(u & 1)


def _i32(u):
    u &= 0xFFFFFFFF
    return u - (1 << 32) if u & 0x80000000 else u


def _i64(u):
    u &= _M64
    return u - (1 << 64) if u & (1 << 63) else u


# ---- raw field parsing -----------------------------------------------------

def parse_fields(buf, limits):
    """Splits `buf` into `(field_id, wire_class, value)` triples.

    `value` is an int for varint/fixed classes and `bytes` for LEN.
    """
    if len(buf) > limits.max_message_bytes:
        raise DecodeError("payload exceeded configured byte limit")
    out = []
    pos = 0
    n = len(buf)
    while pos < n:
        if len(out) >= limits.max_field_count:
            raise DecodeError("maximum field count exceeded")
        tag, pos = read_varint(buf, pos)
        fid, cls = tag >> 3, tag & 3
        if tag & 4:
            raise DecodeError("unknown wire class")
        if fid > 0xFFFFFFFF:
            raise DecodeError("field id out of range")
        if cls == VARINT:
            val, pos = read_varint(buf, pos)
        elif cls == FIXED32:
            if pos + 4 > n:
                raise DecodeError("unexpected end of input (truncated message)")
            val = int.from_bytes(buf[pos:pos + 4], "little")
            pos += 4
        elif cls == FIXED64:
            if pos + 8 > n:
                raise DecodeError("unexpected end of input (truncated message)")
            val = int.from_bytes(buf[pos:pos + 8], "little")
            pos += 8
        else:
            length, pos = read_varint(buf, pos)
            if length > n - pos:
                raise DecodeError("invalid length-delimited length")
            val = bytes(buf[pos:pos + length])
            pos += length
        out.append((fid, cls, val))
    return out


class WireClassMismatch(DecodeError):
    pass


def mismatch(fid):
    return WireClassMismatch("field %d arrived with an unexpected wire class" % fid)


# ---- single values ---------------------------------------------------------

def _to_word(name, v):
    """Scalar -> unsigned wire word (varint/fixed classes)."""
    if name == "bool":
        return 1 if v else 0
    if name in ("int32", "int64"):
        return v & _M64
    if name in ("uint32", "uint64", "fixed32", "fixed64"):
        return v
    if name == "sint32" or name == "sint64":
        return zigzag_encode(v)
    if name == "sfixed32":
        return v & 0xFFFFFFFF
    if name == "sfixed64":
        return v & _M64
    if name == "float32":
        return struct.unpack("<I", struct.pack("<f", v))[0]
    if name == "float64":
        return struct.unpack("<Q", struct.pack("<d", v))[0]
    raise ValueError(name)


def _from_word(name, w):
    if name == "bool":
        return w != 0
    if name == "int32":
        return _i32(w)
    if name == "int64":
        return _i64(w)
    if name == "uint32":
        return w & 0xFFFFFFFF
    if name in ("uint64", "fixed64", "fixed32"):
        return w
    if name == "sint32":
        return _i32(zigzag_decode(w))
    if name == "sint64":
        return _i64(zigzag_decode(w))
    if name == "sfixed32":
        return _i32(w)
    if name == "sfixed64":
        return _i64(w)
    if name == "float32":
        return struct.unpack("<f", struct.pack("<I", w))[0]
    if name == "float64":
        return struct.unpack("<d", struct.pack("<Q", w))[0]
    raise ValueError(name)


def write_word(out, cls, w):
    if cls == VARINT:
        write_varint(out, w)
    elif cls == FIXED32:
        out += (w & 0xFFFFFFFF).to_bytes(4, "little")
    else:
        out += (w & _M64).to_bytes(8, "little")


def write_len(out, fid, payload):
    write_varint(out, (fid << 3) | LEN)
    write_varint(out, len(payload))
    out += payload


def put_scalar(out, fid, name, v):
    """Writes tag + value of a scalar field."""
    cls = _CLASS[name]
    if cls == LEN:
        write_len(out, fid, v.encode("utf-8") if name == "string" else bytes(v))
        return
    write_varint(out, (fid << 3) | cls)
    write_word(out, cls, _to_word(name, v))


def scalar_bytes(name, v):
    """Encoded value without a tag (map entry halves use put_scalar)."""
    out = bytearray()
    cls = _CLASS[name]
    if cls == LEN:
        return v.encode("utf-8") if name == "string" else bytes(v)
    write_word(out, cls, _to_word(name, v))
    return bytes(out)


def get_scalar(fid, cls, value, name, limits):
    """Decodes one scalar field occurrence (class must match exactly)."""
    if cls != _CLASS[name]:
        raise mismatch(fid)
    if cls == LEN:
        if name == "string":
            if len(value) > limits.max_string_bytes:
                raise DecodeError("payload exceeded configured byte limit")
            try:
                return value.decode("utf-8")
            except UnicodeDecodeError:
                raise DecodeError("string field contained invalid UTF-8")
        if len(value) > limits.max_bytes_field_bytes:
            raise DecodeError("payload exceeded configured byte limit")
        return value
    return _from_word(name, value)


# ---- repeated scalars ------------------------------------------------------

def put_packed(out, fid, name, values):
    """Packed repeated scalar (non-empty list of a packable scalar)."""
    cls = _CLASS[name]
    payload = bytearray()
    for v in values:
        write_word(payload, cls, _to_word(name, v))
    write_len(out, fid, bytes(payload))


def get_packed(fid, cls, value, name, limits):
    """Values of one occurrence of a repeated packable field: either a single
    value in the scalar's own class or a packed LEN payload."""
    own = _CLASS[name]
    if cls == own:
        return [_from_word(name, value)]
    if cls != LEN:
        raise mismatch(fid)
    out = []
    pos = 0
    n = len(value)
    if own == VARINT:
        while pos < n:
            w, pos = read_varint(value, pos)
            out.append(_from_word(name, w))
    else:
        size = 4 if own == FIXED32 else 8
        if n % size:
            raise DecodeError("malformed fixed-width scalar")
        for i in range(0, n, size):
            out.append(_from_word(name, int.from_bytes(value[i:i + size], "little")))
    if len(out) > limits.max_repeated_entries:
        raise DecodeError("maximum repeated entries exceeded")
    return out


# ---- enums -----------------------------------------------------------------

def put_enum(out, fid, v):
    write_varint(out, (fid << 3) | VARINT)
    write_varint(out, v & _M64)


def get_enum(fid, cls, value, valid):
    """Decodes an enum value; `valid` is None for open enums, else the set of
    allowed numbers."""
    if cls != VARINT:
        raise mismatch(fid)
    n = _i32(value)
    if valid is not None and n not in valid:
        raise DecodeError("enum value %d is not valid for a closed enum" % n)
    return n


def put_enum_packed(out, fid, values):
    payload = bytearray()
    for v in values:
        write_varint(payload, v & _M64)
    write_len(out, fid, bytes(payload))


def get_enum_packed(fid, cls, value, valid, limits):
    if cls == VARINT:
        return [get_enum(fid, cls, value, valid)]
    if cls != LEN:
        raise mismatch(fid)
    out = []
    pos = 0
    while pos < len(value):
        w, pos = read_varint(value, pos)
        out.append(get_enum(fid, VARINT, w, valid))
    if len(out) > limits.max_repeated_entries:
        raise DecodeError("maximum repeated entries exceeded")
    return out


# ---- messages and maps -----------------------------------------------------

def get_len(fid, cls, value):
    if cls != LEN:
        raise mismatch(fid)
    return value


def check_depth(depth, limits):
    if depth > limits.max_depth:
        raise DecodeError("maximum nesting depth exceeded")


def check_repeated(count, limits):
    if count > limits.max_repeated_entries:
        raise DecodeError("maximum repeated entries exceeded")


def check_map(count, limits):
    if count > limits.max_map_entries:
        raise DecodeError("maximum map entries exceeded")


def sorted_keys(d):
    """Map keys in the order the Rust encoder emits them."""
    return sorted(d)


def map_entry_fields(payload, limits):
    """(key, value) wire fields of a map entry; missing halves are None."""
    key = value = None
    for fid, cls, val in parse_fields(payload, limits):
        if fid == 1:
            key = (cls, val)
        elif fid == 2:
            value = (cls, val)
    return key, value


def put_unknown(out, unknown):
    for fid, cls, val in unknown:
        if cls == LEN:
            write_len(out, fid, val)
        else:
            write_varint(out, (fid << 3) | cls)
            write_word(out, cls, val)


def unknown_size(cls, val):
    return len(val) + 2 if cls == LEN else 10
