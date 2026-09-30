# Performance

The benchmark suite lives in [`benches/`](../benches) (`tpt20-benches`,
criterion) and covers the spec §23 list. Run it with:

```sh
cargo bench -p tpt20-benches                      # everything
cargo bench -p tpt20-benches --bench wire         # wire format, borrowed, dynamic, JSON
cargo bench -p tpt20-benches --bench rpc          # RPC, streaming, TLS, storms
cargo bench -p tpt20-benches -- --warm-up-time 0.5 --measurement-time 1 --noplot   # quick
```

The fixtures are generated from [`benches/schema.tpt`](../benches/schema.tpt)
at build time, so the numbers measure real generated code.

## Reference numbers

Measured on a 4-vCPU Intel Xeon @ 2.8 GHz (shared cloud VM, debug-free
`--release`-equivalent bench profile, short measurement windows — treat as
orders of magnitude, not guarantees; re-run on your hardware).

### Wire format

| Benchmark | Time | Throughput |
|---|---|---|
| small (3 fields) encode / decode | 141 ns / 118 ns | 122 / 146 MiB/s |
| nested (3 levels) encode / decode | 373 ns / 311 ns | 72 / 86 MiB/s |
| repeated (100 strings + 100 int64) encode / decode | 6.3 µs / 12.6 µs | 198 / 98 MiB/s |
| maps (2 × 100 entries) encode / decode | 27 µs / 42 µs | 93 / 61 MiB/s |
| large (1 MiB bytes) encode / decode | 841 µs / 133 µs | 1.2 / 7.4 GiB/s |
| packed int64, 1 000 elements encode / decode | 3.6 µs / 4.3 µs | 277 / 235 Melem/s |
| packed int64, 100 000 elements encode / decode | 467 µs / 557 µs | 214 / 180 Melem/s |
| canonical encode (small / repeated / maps) | 183 ns / 11.8 µs / 54 µs | |

### Borrowed, unknown-field, dynamic, JSON

| Benchmark | Time |
|---|---|
| small decode: owned vs borrowed | 105 ns vs 74 ns |
| nested decode: owned vs borrowed | 292 ns vs 156 ns |
| 1 MiB bytes decode: owned vs borrowed | 133 µs vs 57 ns (zero-copy) |
| unknown fields ×0 / ×10 / ×100 pairs, preserve | 107 ns / 2.3 µs / 18.3 µs |
| same, discard (`RawMessage::decode_filtered`) | 75 ns / 435 ns / 3.3 µs |
| dynamic (`tpt20-reflect`) decode + `get_field` vs generated decode (nested) | 142 ns vs 302 ns |
| JSON small `to_json` / `from_json` | 293 ns / 417 ns |
| JSON maps ×100 `to_json` / `from_json` | 38 µs / 63 µs |

### RPC

| Benchmark | Time |
|---|---|
| unary, in-process | 71 µs |
| unary, HTTP/2 (h2c, pooled connection) | 280 µs |
| unary, HTTP/2 + TLS 1.3 + ALPN (pooled connection) | 281 µs |
| server streaming of 100 messages, in-process / h2c | 181 µs / 609 µs |
| 32 concurrent unary calls, in-process / h2c | 241 µs / 1.5 ms |
| cancellation storm (100 slow calls, all cancelled), in-process | 721 µs |
| deadline storm (100 slow calls, 1 ms deadline), in-process | 2.6 ms |
| compression overhead | not measurable: message compression is not implemented yet |

## Reading the numbers against the spec §23 goals

- **Fast varints / packed fields:** met. Packed `int64` moves 180–280
  million elements per second; 1 MiB `bytes` decodes at ~7 GiB/s.
- **Optional zero-copy decoding:** met. Borrowed views are 30–50 % faster on
  small messages and effectively free (tens of ns) for large payloads.
- **Monomorphized generated code:** yes — generated encode/decode/JSON are
  concrete per message; there is no per-field dynamic dispatch. Dynamic
  decoding through `tpt20-reflect` is lazy and therefore cheaper than a full
  typed decode for field-at-a-time access.
- **Bounded memory:** enforced by `DecoderLimits` on every decode path
  (see [Security limits](security-limits.md)); not a throughput property.
- **Efficient maps / repeated:** acceptable, but the weakest area (see
  backlog): map decode is ~4× slower per byte than packed numerics.
- **Efficient streaming:** per-message overhead is ~1 µs in-process and
  ~4 µs over HTTP/2 once a stream is open. `Http2Transport` multiplexes all
  calls over one pooled connection, so TLS costs a handshake once, not per
  call (unary over TLS is now indistinguishable from plain h2c).
- **Low-overhead observability:** the `tpt20-observability` hooks are not
  on the benchmarked paths, so this goal is **not yet measured** here.

## Optimization backlog (from the profiling pass)

Ordered by expected payoff. None of these are correctness issues.

1. ~~HTTP/2 connection reuse~~ — **done.** The transport keeps one
   multiplexed connection (per `Http2Transport` and its clones), opens a
   stream per call, and transparently reconnects once if the cached connection
   has died. Effect: unary h2c 462 → 280 µs, unary TLS 1.29 ms → 281 µs, 32
   concurrent calls 4.1 → 1.5 ms. (The numbers above were re-measured after
   the change.)
2. ~~Canonical encoding~~ — **done.** Sorting used to allocate a payload
   key on every comparison and clone every field; it now sorts references
   with allocation-free keys and moves map entries instead of cloning them.
   Canonical encode: repeated 36.7 → 11.8 µs (3.1×), maps 93 → 54 µs (1.7×),
   small 251 → 183 ns. The order is unchanged (`field_id, wire_class,
   payload`), so canonical bytes are identical to before.
3. **Unknown-field preservation** (partly done). The generated decoder now
   moves unknown fields out of the decoded `RawMessage` instead of cloning
   them (100 pairs: 23.7 → 18.3 µs). The remaining gap to the discard path
   (3.3 µs) is one allocation per length-delimited unknown field; storing
   unknowns as spans of one shared buffer would remove it but changes the
   public `unknown_fields` type.
4. ~~Map decode~~ — **done.** Entries are parsed with the borrowed decoder
   (no per-entry `RawMessage`/payload copies): maps ×100 decode 58 → 42 µs.
5. **Per-call timers in the RPC runtime.** Every call arms a `tokio` sleep for
   the deadline in both `Channel` and `Server`; in-process unary is 71 µs
   mostly from task/timer/mutex overhead rather than encoding (~0.3 µs).
   Sharing one timer wheel entry per call and avoiding the `Mutex` around the
   response sender on the unary fast path are the obvious cuts.
6. **Compression.** Not implemented; once added it needs its own benchmark
   (spec §23 lists compression overhead).
