// tpt20 minimal Java runtime: native wire format, decoder limits, errors.
//
// Generated together with the message code; do not edit. Implements the same
// format and the same limits as the Rust runtime (spec sections 9 and 18).

package PACKAGE;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.function.IntPredicate;

public final class Tpt20Runtime {
    private Tpt20Runtime() {}

    /** Malformed or limit-violating input. */
    public static final class DecodeException extends RuntimeException {
        public DecodeException(String message) {
            super("tpt20: " + message);
        }
    }

    /** Bounds enforced while decoding untrusted input (same defaults as Rust). */
    public static final class Limits {
        public int maxMessageBytes = 4 * 1024 * 1024;
        public int maxDepth = 100;
        public int maxFieldCount = 32 * 1024;
        public int maxUnknownFieldBytes = 4 * 1024 * 1024;
        public int maxStringBytes = 4 * 1024 * 1024;
        public int maxBytesFieldBytes = 16 * 1024 * 1024;
        public int maxRepeatedEntries = 512 * 1024;
        public int maxMapEntries = 512 * 1024;
    }

    public static Limits defaultLimits() {
        return new Limits();
    }

    public static final int VARINT = 0, FIXED32 = 1, FIXED64 = 2, LEN = 3;

    /** One field as found on the wire. */
    public static final class RawField {
        public final long id;
        public final int cls;
        public final long word;
        public final byte[] bytes;

        RawField(long id, int cls, long word, byte[] bytes) {
            this.id = id;
            this.cls = cls;
            this.word = word;
            this.bytes = bytes;
        }
    }

    /** Scalar field types. */
    public enum Scalar {
        BOOL(VARINT), INT32(VARINT), INT64(VARINT), UINT32(VARINT), UINT64(VARINT),
        SINT32(VARINT), SINT64(VARINT), FIXED32(Tpt20Runtime.FIXED32), SFIXED32(Tpt20Runtime.FIXED32),
        FLOAT32(Tpt20Runtime.FIXED32), FIXED64(Tpt20Runtime.FIXED64), SFIXED64(Tpt20Runtime.FIXED64),
        FLOAT64(Tpt20Runtime.FIXED64), STRING(LEN), BYTES(LEN);

        final int cls;

        Scalar(int cls) {
            this.cls = cls;
        }

        boolean packable() {
            return cls != LEN;
        }

        long toWord(Object v) {
            switch (this) {
                case BOOL: return ((Boolean) v) ? 1 : 0;
                case INT32: return (long) (Integer) v;
                case INT64: return (Long) v;
                case UINT32: case FIXED32: return ((Integer) v) & 0xFFFFFFFFL;
                case UINT64: case FIXED64: case SFIXED64: return (Long) v;
                case SINT32: { long n = (Integer) v; return (n << 1) ^ (n >> 63); }
                case SINT64: { long n = (Long) v; return (n << 1) ^ (n >> 63); }
                case SFIXED32: return ((Integer) v) & 0xFFFFFFFFL;
                case FLOAT32: return Float.floatToRawIntBits((Float) v) & 0xFFFFFFFFL;
                case FLOAT64: return Double.doubleToRawLongBits((Double) v);
                default: throw new IllegalStateException();
            }
        }

        Object fromWord(long w) {
            switch (this) {
                case BOOL: return w != 0;
                case INT32: case UINT32: case FIXED32: case SFIXED32: return (int) w;
                case INT64: case UINT64: case FIXED64: case SFIXED64: return w;
                case SINT32: return (int) ((w >>> 1) ^ -(w & 1));
                case SINT64: return (w >>> 1) ^ -(w & 1);
                case FLOAT32: return Float.intBitsToFloat((int) w);
                case FLOAT64: return Double.longBitsToDouble(w);
                default: throw new IllegalStateException();
            }
        }
    }

    // ---- parsing -------------------------------------------------------------

    static final class Pos {
        int v;
    }

    static long readVarint(byte[] b, Pos p) {
        long result = 0;
        for (int i = 0; i < 10; i++) {
            if (p.v >= b.length) {
                throw new DecodeException("unexpected end of input (truncated message)");
            }
            int c = b[p.v++] & 0xFF;
            if (i == 9 && c > 1) {
                throw new DecodeException("varint too long (would overflow 64 bits)");
            }
            result |= (long) (c & 0x7F) << (7 * i);
            if ((c & 0x80) == 0) {
                return result;
            }
        }
        throw new DecodeException("varint too long (would overflow 64 bits)");
    }

    public static List<RawField> parseFields(byte[] b, Limits l) {
        if (b.length > l.maxMessageBytes) {
            throw new DecodeException("payload exceeded configured byte limit");
        }
        List<RawField> out = new ArrayList<>();
        Pos p = new Pos();
        while (p.v < b.length) {
            if (out.size() >= l.maxFieldCount) {
                throw new DecodeException("maximum field count exceeded");
            }
            long tag = readVarint(b, p);
            if ((tag & 4) != 0) {
                throw new DecodeException("unknown wire class");
            }
            long id = tag >>> 3;
            if (id > 0xFFFFFFFFL) {
                throw new DecodeException("field id out of range");
            }
            int cls = (int) (tag & 3);
            switch (cls) {
                case VARINT:
                    out.add(new RawField(id, cls, readVarint(b, p), null));
                    break;
                case FIXED32: {
                    if (p.v + 4 > b.length) {
                        throw new DecodeException("unexpected end of input (truncated message)");
                    }
                    long w = ByteBuffer.wrap(b, p.v, 4).order(ByteOrder.LITTLE_ENDIAN).getInt() & 0xFFFFFFFFL;
                    p.v += 4;
                    out.add(new RawField(id, cls, w, null));
                    break;
                }
                case FIXED64: {
                    if (p.v + 8 > b.length) {
                        throw new DecodeException("unexpected end of input (truncated message)");
                    }
                    long w = ByteBuffer.wrap(b, p.v, 8).order(ByteOrder.LITTLE_ENDIAN).getLong();
                    p.v += 8;
                    out.add(new RawField(id, cls, w, null));
                    break;
                }
                default: {
                    long n = readVarint(b, p);
                    if (n < 0 || n > b.length - p.v) {
                        throw new DecodeException("invalid length-delimited length");
                    }
                    out.add(new RawField(id, cls, 0, Arrays.copyOfRange(b, p.v, p.v + (int) n)));
                    p.v += (int) n;
                }
            }
        }
        return out;
    }

    static DecodeException mismatch(RawField f) {
        return new DecodeException("field " + f.id + " arrived with an unexpected wire class");
    }

    // ---- writer --------------------------------------------------------------

    /** Accumulates encoded bytes. */
    public static final class Writer {
        private byte[] buf = new byte[64];
        private int len;

        private void ensure(int extra) {
            if (len + extra > buf.length) {
                buf = Arrays.copyOf(buf, Math.max(buf.length * 2, len + extra));
            }
        }

        void varint(long v) {
            ensure(10);
            while ((v & ~0x7FL) != 0) {
                buf[len++] = (byte) ((v & 0x7F) | 0x80);
                v >>>= 7;
            }
            buf[len++] = (byte) v;
        }

        void word(int cls, long v) {
            if (cls == VARINT) {
                varint(v);
            } else {
                int n = cls == FIXED32 ? 4 : 8;
                ensure(n);
                for (int i = 0; i < n; i++) {
                    buf[len++] = (byte) (v >>> (8 * i));
                }
            }
        }

        void tag(long id, int cls) {
            varint((id << 3) | cls);
        }

        public void putLen(long id, byte[] payload) {
            tag(id, LEN);
            varint(payload.length);
            ensure(payload.length);
            System.arraycopy(payload, 0, buf, len, payload.length);
            len += payload.length;
        }

        public void putScalar(long id, Scalar s, Object v) {
            if (s == Scalar.STRING) {
                putLen(id, ((String) v).getBytes(StandardCharsets.UTF_8));
            } else if (s == Scalar.BYTES) {
                putLen(id, (byte[]) v);
            } else {
                tag(id, s.cls);
                word(s.cls, s.toWord(v));
            }
        }

        public void putPacked(long id, Scalar s, List<?> values) {
            Writer p = new Writer();
            for (Object v : values) {
                p.word(s.cls, s.toWord(v));
            }
            putLen(id, p.toByteArray());
        }

        public void putEnum(long id, int v) {
            tag(id, VARINT);
            varint((long) v);
        }

        public void putEnumPacked(long id, List<Integer> values) {
            Writer p = new Writer();
            for (int v : values) {
                p.varint((long) v);
            }
            putLen(id, p.toByteArray());
        }

        public void putUnknown(List<RawField> unknown) {
            for (RawField f : unknown) {
                if (f.cls == LEN) {
                    putLen(f.id, f.bytes);
                } else {
                    tag(f.id, f.cls);
                    word(f.cls, f.word);
                }
            }
        }

        public byte[] toByteArray() {
            return Arrays.copyOf(buf, len);
        }
    }

    // ---- decoding helpers ----------------------------------------------------

    public static Object getScalar(RawField f, Scalar s, Limits l) {
        if (f.cls != s.cls) {
            throw mismatch(f);
        }
        if (s == Scalar.STRING) {
            if (f.bytes.length > l.maxStringBytes) {
                throw new DecodeException("payload exceeded configured byte limit");
            }
            try {
                return StandardCharsets.UTF_8.newDecoder()
                        .onMalformedInput(CodingErrorAction.REPORT)
                        .onUnmappableCharacter(CodingErrorAction.REPORT)
                        .decode(ByteBuffer.wrap(f.bytes))
                        .toString();
            } catch (CharacterCodingException e) {
                throw new DecodeException("string field contained invalid UTF-8");
            }
        }
        if (s == Scalar.BYTES) {
            if (f.bytes.length > l.maxBytesFieldBytes) {
                throw new DecodeException("payload exceeded configured byte limit");
            }
            return f.bytes.clone();
        }
        return s.fromWord(f.word);
    }

    /** One occurrence of a repeated packable field: a single value or a packed payload. */
    public static List<Object> getPacked(RawField f, Scalar s, Limits l) {
        List<Object> out = new ArrayList<>();
        if (f.cls == s.cls) {
            out.add(s.fromWord(f.word));
            return out;
        }
        if (f.cls != LEN) {
            throw mismatch(f);
        }
        byte[] b = f.bytes;
        if (s.cls == VARINT) {
            Pos p = new Pos();
            while (p.v < b.length) {
                out.add(s.fromWord(readVarint(b, p)));
            }
        } else {
            int size = s.cls == FIXED32 ? 4 : 8;
            if (b.length % size != 0) {
                throw new DecodeException("malformed fixed-width scalar");
            }
            ByteBuffer bb = ByteBuffer.wrap(b).order(ByteOrder.LITTLE_ENDIAN);
            for (int i = 0; i < b.length; i += size) {
                out.add(s.fromWord(size == 4 ? bb.getInt(i) & 0xFFFFFFFFL : bb.getLong(i)));
            }
        }
        if (out.size() > l.maxRepeatedEntries) {
            throw new DecodeException("maximum repeated entries exceeded");
        }
        return out;
    }

    public static byte[] getLen(RawField f) {
        if (f.cls != LEN) {
            throw mismatch(f);
        }
        return f.bytes;
    }

    /** Decodes an enum occurrence; {@code valid} is null for open enums. */
    public static int getEnum(RawField f, IntPredicate valid) {
        if (f.cls != VARINT) {
            throw mismatch(f);
        }
        int n = (int) f.word;
        if (valid != null && !valid.test(n)) {
            throw new DecodeException("enum value " + n + " is not valid for a closed enum");
        }
        return n;
    }

    public static List<Integer> getEnumPacked(RawField f, IntPredicate valid, Limits l) {
        List<Integer> out = new ArrayList<>();
        if (f.cls == VARINT) {
            out.add(getEnum(f, valid));
            return out;
        }
        if (f.cls != LEN) {
            throw mismatch(f);
        }
        Pos p = new Pos();
        while (p.v < f.bytes.length) {
            long w = readVarint(f.bytes, p);
            out.add(getEnum(new RawField(f.id, VARINT, w, null), valid));
        }
        if (out.size() > l.maxRepeatedEntries) {
            throw new DecodeException("maximum repeated entries exceeded");
        }
        return out;
    }

    /** {key field, value field} of a map entry; a missing half is null. */
    public static RawField[] mapEntry(byte[] payload, Limits l) {
        RawField[] kv = new RawField[2];
        for (RawField f : parseFields(payload, l)) {
            if (f.id == 1) {
                kv[0] = f;
            } else if (f.id == 2) {
                kv[1] = f;
            }
        }
        return kv;
    }

    public static void checkDepth(int depth, Limits l) {
        if (depth > l.maxDepth) {
            throw new DecodeException("maximum nesting depth exceeded");
        }
    }

    public static void checkRepeated(int n, Limits l) {
        if (n > l.maxRepeatedEntries) {
            throw new DecodeException("maximum repeated entries exceeded");
        }
    }

    public static void checkMap(int n, Limits l) {
        if (n > l.maxMapEntries) {
            throw new DecodeException("maximum map entries exceeded");
        }
    }

    public static int unknownSize(RawField f) {
        return f.cls == LEN ? f.bytes.length + 2 : 10;
    }

    // ---- map ordering ----------------------------------------------------------

    /** Map keys in the order the Rust encoder emits them. */
    @SuppressWarnings("unchecked")
    public static <K> List<K> sortedKeys(Map<K, ?> m, Scalar keyType) {
        List<K> keys = new ArrayList<>(m.keySet());
        Comparator<Object> cmp;
        switch (keyType) {
            case STRING:
                cmp = (a, b) -> Arrays.compareUnsigned(
                        ((String) a).getBytes(StandardCharsets.UTF_8),
                        ((String) b).getBytes(StandardCharsets.UTF_8));
                break;
            case BOOL:
                cmp = (a, b) -> Boolean.compare((Boolean) a, (Boolean) b);
                break;
            case UINT32: case FIXED32:
                cmp = (a, b) -> Integer.compareUnsigned((Integer) a, (Integer) b);
                break;
            case UINT64: case FIXED64:
                cmp = (a, b) -> Long.compareUnsigned((Long) a, (Long) b);
                break;
            case INT32: case SINT32: case SFIXED32:
                cmp = (a, b) -> Integer.compare((Integer) a, (Integer) b);
                break;
            default:
                cmp = (a, b) -> Long.compare((Long) a, (Long) b);
        }
        keys.sort((Comparator<K>) (Comparator<?>) cmp);
        return keys;
    }
}
