// tpt20 minimal Go runtime: native wire format, decoder limits, errors.
//
// Generated together with the message code; do not edit. Implements the same
// format and the same limits as the Rust runtime (spec sections 9 and 18).

package PACKAGE

import (
	"cmp"
	"errors"
	"fmt"
	"math"
	"slices"
	"unicode/utf8"
)

// Tpt20DecodeError reports malformed or limit-violating input.
type Tpt20DecodeError struct{ msg string }

func (e *Tpt20DecodeError) Error() string { return "tpt20: " + e.msg }

func rtErr(format string, args ...any) error {
	return &Tpt20DecodeError{fmt.Sprintf(format, args...)}
}

// IsTpt20DecodeError reports whether err is a decode error.
func IsTpt20DecodeError(err error) bool {
	var d *Tpt20DecodeError
	return errors.As(err, &d)
}

// Tpt20Limits bounds what the decoder accepts from untrusted input.
type Tpt20Limits struct {
	MaxMessageBytes      int
	MaxDepth             int
	MaxFieldCount        int
	MaxUnknownFieldBytes int
	MaxStringBytes       int
	MaxBytesFieldBytes   int
	MaxRepeatedEntries   int
	MaxMapEntries        int
}

// Tpt20DefaultLimits returns the same defaults as the Rust runtime.
func Tpt20DefaultLimits() *Tpt20Limits {
	return &Tpt20Limits{
		MaxMessageBytes:      4 * 1024 * 1024,
		MaxDepth:             100,
		MaxFieldCount:        32 * 1024,
		MaxUnknownFieldBytes: 4 * 1024 * 1024,
		MaxStringBytes:       4 * 1024 * 1024,
		MaxBytesFieldBytes:   16 * 1024 * 1024,
		MaxRepeatedEntries:   512 * 1024,
		MaxMapEntries:        512 * 1024,
	}
}

// Wire classes.
const (
	wcVarint  uint8 = 0
	wcFixed32 uint8 = 1
	wcFixed64 uint8 = 2
	wcLen     uint8 = 3
)

// Tpt20RawField is one field as found on the wire. Word holds varint and
// fixed values; Bytes holds length-delimited payloads.
type Tpt20RawField struct {
	ID    uint32
	Class uint8
	Word  uint64
	Bytes []byte
}

func rtReadVarint(b []byte, pos int) (uint64, int, error) {
	var result uint64
	for i := 0; i < 10; i++ {
		if pos >= len(b) {
			return 0, pos, rtErr("unexpected end of input (truncated message)")
		}
		c := b[pos]
		pos++
		if i == 9 && c > 1 {
			return 0, pos, rtErr("varint too long (would overflow 64 bits)")
		}
		result |= uint64(c&0x7f) << (7 * uint(i))
		if c&0x80 == 0 {
			return result, pos, nil
		}
	}
	return 0, pos, rtErr("varint too long (would overflow 64 bits)")
}

func rtParseFields(b []byte, l *Tpt20Limits) ([]Tpt20RawField, error) {
	if len(b) > l.MaxMessageBytes {
		return nil, rtErr("payload exceeded configured byte limit")
	}
	var out []Tpt20RawField
	pos := 0
	for pos < len(b) {
		if len(out) >= l.MaxFieldCount {
			return nil, rtErr("maximum field count exceeded")
		}
		tag, p, err := rtReadVarint(b, pos)
		if err != nil {
			return nil, err
		}
		pos = p
		if tag&4 != 0 {
			return nil, rtErr("unknown wire class")
		}
		if tag>>3 > math.MaxUint32 {
			return nil, rtErr("field id out of range")
		}
		f := Tpt20RawField{ID: uint32(tag >> 3), Class: uint8(tag & 3)}
		switch f.Class {
		case wcVarint:
			f.Word, pos, err = rtReadVarint(b, pos)
			if err != nil {
				return nil, err
			}
		case wcFixed32:
			if pos+4 > len(b) {
				return nil, rtErr("unexpected end of input (truncated message)")
			}
			f.Word = uint64(b[pos]) | uint64(b[pos+1])<<8 | uint64(b[pos+2])<<16 | uint64(b[pos+3])<<24
			pos += 4
		case wcFixed64:
			if pos+8 > len(b) {
				return nil, rtErr("unexpected end of input (truncated message)")
			}
			for i := 7; i >= 0; i-- {
				f.Word = f.Word<<8 | uint64(b[pos+i])
			}
			pos += 8
		default:
			n, p, err := rtReadVarint(b, pos)
			if err != nil {
				return nil, err
			}
			pos = p
			if n > uint64(len(b)-pos) {
				return nil, rtErr("invalid length-delimited length")
			}
			f.Bytes = b[pos : pos+int(n)]
			pos += int(n)
		}
		out = append(out, f)
	}
	return out, nil
}

func rtMismatch(id uint32) error {
	return rtErr("field %d arrived with an unexpected wire class", id)
}

// Tpt20Writer accumulates encoded bytes.
type Tpt20Writer struct{ buf []byte }

// Bytes returns the encoded bytes.
func (w *Tpt20Writer) Bytes() []byte { return w.buf }

func (w *Tpt20Writer) varint(v uint64) {
	for v >= 0x80 {
		w.buf = append(w.buf, byte(v)|0x80)
		v >>= 7
	}
	w.buf = append(w.buf, byte(v))
}

func (w *Tpt20Writer) tag(id uint32, class uint8) { w.varint(uint64(id)<<3 | uint64(class)) }

func (w *Tpt20Writer) word(class uint8, v uint64) {
	switch class {
	case wcVarint:
		w.varint(v)
	case wcFixed32:
		w.buf = append(w.buf, byte(v), byte(v>>8), byte(v>>16), byte(v>>24))
	default:
		w.buf = append(w.buf, byte(v), byte(v>>8), byte(v>>16), byte(v>>24),
			byte(v>>32), byte(v>>40), byte(v>>48), byte(v>>56))
	}
}

// PutLen writes a length-delimited field.
func (w *Tpt20Writer) PutLen(id uint32, payload []byte) {
	w.tag(id, wcLen)
	w.varint(uint64(len(payload)))
	w.buf = append(w.buf, payload...)
}

// PutWord writes a varint/fixed field.
func (w *Tpt20Writer) PutWord(id uint32, class uint8, v uint64) {
	w.tag(id, class)
	w.word(class, v)
}

// PutEnum writes an enum value (sign-extended varint).
func (w *Tpt20Writer) PutEnum(id uint32, v int32) { w.PutWord(id, wcVarint, uint64(int64(v))) }

// PutUnknown re-emits preserved unknown fields.
func (w *Tpt20Writer) PutUnknown(fs []Tpt20RawField) {
	for _, f := range fs {
		if f.Class == wcLen {
			w.PutLen(f.ID, f.Bytes)
		} else {
			w.PutWord(f.ID, f.Class, f.Word)
		}
	}
}

// ---- scalar <-> word conversions ------------------------------------------

func zigzagEnc(n int64) uint64 { return uint64(n<<1) ^ uint64(n>>63) }
func zigzagDec(u uint64) int64 { return int64(u>>1) ^ -int64(u&1) }

func toWBool(v bool) uint64 {
	if v {
		return 1
	}
	return 0
}
func toWInt32(v int32) uint64     { return uint64(int64(v)) }
func toWInt64(v int64) uint64     { return uint64(v) }
func toWUint32(v uint32) uint64   { return uint64(v) }
func toWUint64(v uint64) uint64   { return v }
func toWSint32(v int32) uint64    { return zigzagEnc(int64(v)) }
func toWSint64(v int64) uint64    { return zigzagEnc(v) }
func toWFixed32(v uint32) uint64  { return uint64(v) }
func toWFixed64(v uint64) uint64  { return v }
func toWSfixed32(v int32) uint64  { return uint64(uint32(v)) }
func toWSfixed64(v int64) uint64  { return uint64(v) }
func toWFloat32(v float32) uint64 { return uint64(math.Float32bits(v)) }
func toWFloat64(v float64) uint64 { return math.Float64bits(v) }

func fromWBool(w uint64) bool      { return w != 0 }
func fromWInt32(w uint64) int32    { return int32(uint32(w)) }
func fromWInt64(w uint64) int64    { return int64(w) }
func fromWUint32(w uint64) uint32  { return uint32(w) }
func fromWUint64(w uint64) uint64  { return w }
func fromWSint32(w uint64) int32   { return int32(zigzagDec(w)) }
func fromWSint64(w uint64) int64   { return zigzagDec(w) }
func fromWFixed32(w uint64) uint32 { return uint32(w) }
func fromWFixed64(w uint64) uint64 { return w }
func fromWSfixed32(w uint64) int32 { return int32(uint32(w)) }
func fromWSfixed64(w uint64) int64 { return int64(w) }
func fromWFloat32(w uint64) float32 {
	return math.Float32frombits(uint32(w))
}
func fromWFloat64(w uint64) float64 { return math.Float64frombits(w) }

// rtGetWord decodes a single varint/fixed occurrence of exactly `class`.
func rtGetWord[T any](f Tpt20RawField, class uint8, from func(uint64) T) (T, error) {
	var zero T
	if f.Class != class {
		return zero, rtMismatch(f.ID)
	}
	return from(f.Word), nil
}

// rtPutPacked writes a packed repeated scalar.
func rtPutPacked[T any](w *Tpt20Writer, id uint32, class uint8, vs []T, to func(T) uint64) {
	var p Tpt20Writer
	for _, v := range vs {
		p.word(class, to(v))
	}
	w.PutLen(id, p.buf)
}

// rtGetPacked decodes one occurrence of a repeated packable field: a single
// value in the scalar's own class, or a packed length-delimited payload.
func rtGetPacked[T any](f Tpt20RawField, class uint8, from func(uint64) T, l *Tpt20Limits) ([]T, error) {
	if f.Class == class {
		return []T{from(f.Word)}, nil
	}
	if f.Class != wcLen {
		return nil, rtMismatch(f.ID)
	}
	var out []T
	b := f.Bytes
	switch class {
	case wcVarint:
		pos := 0
		for pos < len(b) {
			w, p, err := rtReadVarint(b, pos)
			if err != nil {
				return nil, err
			}
			pos = p
			out = append(out, from(w))
		}
	default:
		size := 4
		if class == wcFixed64 {
			size = 8
		}
		if len(b)%size != 0 {
			return nil, rtErr("malformed fixed-width scalar")
		}
		for i := 0; i < len(b); i += size {
			var w uint64
			for j := size - 1; j >= 0; j-- {
				w = w<<8 | uint64(b[i+j])
			}
			out = append(out, from(w))
		}
	}
	if len(out) > l.MaxRepeatedEntries {
		return nil, rtErr("maximum repeated entries exceeded")
	}
	return out, nil
}

// ---- strings, bytes, lengths ----------------------------------------------

// Tpt20GetString decodes a string field (UTF-8 validated).
func Tpt20GetString(f Tpt20RawField, l *Tpt20Limits) (string, error) {
	if f.Class != wcLen {
		return "", rtMismatch(f.ID)
	}
	if len(f.Bytes) > l.MaxStringBytes {
		return "", rtErr("payload exceeded configured byte limit")
	}
	if !utf8.Valid(f.Bytes) {
		return "", rtErr("string field contained invalid UTF-8")
	}
	return string(f.Bytes), nil
}

// Tpt20GetBytes decodes a bytes field (copied).
func Tpt20GetBytes(f Tpt20RawField, l *Tpt20Limits) ([]byte, error) {
	if f.Class != wcLen {
		return nil, rtMismatch(f.ID)
	}
	if len(f.Bytes) > l.MaxBytesFieldBytes {
		return nil, rtErr("payload exceeded configured byte limit")
	}
	return append([]byte{}, f.Bytes...), nil
}

// Tpt20GetLen returns the payload of a length-delimited field.
func Tpt20GetLen(f Tpt20RawField) ([]byte, error) {
	if f.Class != wcLen {
		return nil, rtMismatch(f.ID)
	}
	return f.Bytes, nil
}

// ---- enums -----------------------------------------------------------------

// Tpt20GetEnum decodes an enum occurrence; valid is nil for open enums.
func Tpt20GetEnum(f Tpt20RawField, valid func(int32) bool) (int32, error) {
	if f.Class != wcVarint {
		return 0, rtMismatch(f.ID)
	}
	n := int32(uint32(f.Word))
	if valid != nil && !valid(n) {
		return 0, rtErr("enum value %d is not valid for a closed enum", n)
	}
	return n, nil
}

// Tpt20GetEnumPacked decodes one occurrence of a repeated enum field.
func Tpt20GetEnumPacked(f Tpt20RawField, valid func(int32) bool, l *Tpt20Limits) ([]int32, error) {
	if f.Class == wcVarint {
		n, err := Tpt20GetEnum(f, valid)
		return []int32{n}, err
	}
	if f.Class != wcLen {
		return nil, rtMismatch(f.ID)
	}
	var out []int32
	pos := 0
	for pos < len(f.Bytes) {
		w, p, err := rtReadVarint(f.Bytes, pos)
		if err != nil {
			return nil, err
		}
		pos = p
		n, err := Tpt20GetEnum(Tpt20RawField{ID: f.ID, Class: wcVarint, Word: w}, valid)
		if err != nil {
			return nil, err
		}
		out = append(out, n)
	}
	if len(out) > l.MaxRepeatedEntries {
		return nil, rtErr("maximum repeated entries exceeded")
	}
	return out, nil
}

// Tpt20PutEnumPacked writes a packed repeated enum.
func Tpt20PutEnumPacked[E ~int32](w *Tpt20Writer, id uint32, vs []E) {
	var p Tpt20Writer
	for _, v := range vs {
		p.varint(uint64(int64(int32(v))))
	}
	w.PutLen(id, p.buf)
}

// ---- maps ------------------------------------------------------------------

// rtMapKeys returns the keys in the order the Rust encoder emits them.
func rtMapKeys[K cmp.Ordered, V any](m map[K]V) []K {
	keys := make([]K, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	return keys
}

func rtMapKeysBool[V any](m map[bool]V) []bool {
	var keys []bool
	if _, ok := m[false]; ok {
		keys = append(keys, false)
	}
	if _, ok := m[true]; ok {
		keys = append(keys, true)
	}
	return keys
}

// Tpt20MapEntry returns the key/value fields of a map entry (nil if absent).
func Tpt20MapEntry(payload []byte, l *Tpt20Limits) (key, value *Tpt20RawField, err error) {
	fs, err := rtParseFields(payload, l)
	if err != nil {
		return nil, nil, err
	}
	for i := range fs {
		switch fs[i].ID {
		case 1:
			key = &fs[i]
		case 2:
			value = &fs[i]
		}
	}
	return key, value, nil
}

func rtCheckDepth(depth int, l *Tpt20Limits) error {
	if depth > l.MaxDepth {
		return rtErr("maximum nesting depth exceeded")
	}
	return nil
}

func rtCheckRepeated(n int, l *Tpt20Limits) error {
	if n > l.MaxRepeatedEntries {
		return rtErr("maximum repeated entries exceeded")
	}
	return nil
}

func rtCheckMap(n int, l *Tpt20Limits) error {
	if n > l.MaxMapEntries {
		return rtErr("maximum map entries exceeded")
	}
	return nil
}

func rtUnknownSize(f Tpt20RawField) int {
	if f.Class == wcLen {
		return len(f.Bytes) + 2
	}
	return 10
}

// rtEnumOf decodes an enum occurrence into the caller's enum type.
func rtEnumOf[E ~int32](f Tpt20RawField, valid func(int32) bool) (E, error) {
	n, err := Tpt20GetEnum(f, valid)
	return E(n), err
}

// rtMessageOf decodes a message-valued map entry half.
func rtMessageOf[T any](f Tpt20RawField, l *Tpt20Limits, depth int, dec func([]byte) (*T, error)) (*T, error) {
	b, err := Tpt20GetLen(f)
	if err != nil {
		return nil, err
	}
	return dec(b)
}
