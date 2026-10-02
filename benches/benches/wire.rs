//! Wire-format, borrowed, dynamic and JSON benchmarks.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use tpt20_benches::generated::*;
use tpt20_core::{DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

fn small() -> Small {
    Small {
        id: 123_456_789,
        name: "benchmark".into(),
        flag: true,
        ..Default::default()
    }
}

fn nested() -> Nested {
    Nested {
        mid: Some(Mid {
            leaf: Some(Leaf {
                v: 42,
                s: "leaf value".into(),
                ..Default::default()
            }),
            n: 7,
            ..Default::default()
        }),
        tag: "nested".into(),
        ..Default::default()
    }
}

fn repeated(n: usize) -> Repeated {
    Repeated {
        items: (0..n).map(|i| format!("item-{i}")).collect(),
        nums: (0..n as i64).map(|i| i * 1_000_003 - 500).collect(),
        ..Default::default()
    }
}

fn maps(n: usize) -> Maps {
    Maps {
        attrs: (0..n)
            .map(|i| (format!("key-{i}"), format!("value-{i}")))
            .collect(),
        counts: (0..n as i64).map(|i| (i, i * i)).collect(),
        ..Default::default()
    }
}

fn big(bytes: usize) -> Big {
    Big {
        blob: (0..bytes).map(|i| (i % 251) as u8).collect(),
        name: "big".into(),
        ..Default::default()
    }
}

/// `Small` with extra fields the schema does not know about.
fn with_unknown(n: u32) -> Vec<u8> {
    let mut raw = small().to_raw();
    for i in 0..n {
        raw.push(Field::new(
            100 + i,
            WireClass::Varint,
            Value::Varint(u64::from(i)),
        ));
        raw.push(Field::new(
            200 + i,
            WireClass::Len,
            Value::Len(vec![b'x'; 16]),
        ));
    }
    raw.encode().unwrap()
}

macro_rules! encode_decode {
    ($c:expr, $name:expr, $ty:ty, $value:expr) => {{
        let value: $ty = $value;
        let bytes = value.encode();
        let mut g = $c.benchmark_group($name);
        g.throughput(Throughput::Bytes(bytes.len() as u64));
        g.bench_function("encode", |b| b.iter(|| black_box(&value).encode()));
        g.bench_function("encode_canonical", |b| {
            b.iter(|| black_box(&value).encode_canonical())
        });
        g.bench_function("decode", |b| {
            b.iter(|| <$ty>::decode(black_box(&bytes)).unwrap())
        });
        g.finish();
    }};
}

fn messages(c: &mut Criterion) {
    encode_decode!(c, "small", Small, small());
    encode_decode!(c, "nested", Nested, nested());
    encode_decode!(c, "repeated_100", Repeated, repeated(100));
    encode_decode!(c, "maps_100", Maps, maps(100));
    encode_decode!(c, "large_1MiB", Big, big(1 << 20));
}

fn packed(c: &mut Criterion) {
    let mut g = c.benchmark_group("packed_int64");
    for n in [10usize, 1_000, 100_000] {
        let msg = Repeated {
            nums: (0..n as i64).map(|i| i * 77).collect(),
            ..Default::default()
        };
        let bytes = msg.encode();
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("encode", n), &msg, |b, m| {
            b.iter(|| m.encode())
        });
        g.bench_with_input(BenchmarkId::new("decode", n), &bytes, |b, by| {
            b.iter(|| Repeated::decode(black_box(by)).unwrap())
        });
    }
    g.finish();
}

fn borrowed(c: &mut Criterion) {
    let mut g = c.benchmark_group("borrowed_vs_owned");
    let s = small().encode();
    g.bench_function("small/owned", |b| {
        b.iter(|| Small::decode(black_box(&s)).unwrap())
    });
    g.bench_function("small/borrowed", |b| {
        b.iter(|| Small::decode_borrowed(black_box(&s)).unwrap())
    });
    let n = nested().encode();
    g.bench_function("nested/owned", |b| {
        b.iter(|| Nested::decode(black_box(&n)).unwrap())
    });
    g.bench_function("nested/borrowed", |b| {
        b.iter(|| Nested::decode_borrowed(black_box(&n)).unwrap())
    });
    let big_bytes = big(1 << 20).encode();
    g.throughput(Throughput::Bytes(big_bytes.len() as u64));
    g.bench_function("large_1MiB/owned", |b| {
        b.iter(|| Big::decode(black_box(&big_bytes)).unwrap())
    });
    g.bench_function("large_1MiB/borrowed", |b| {
        b.iter(|| Big::decode_borrowed(black_box(&big_bytes)).unwrap())
    });
    g.finish();
}

fn unknown_fields(c: &mut Criterion) {
    let mut g = c.benchmark_group("unknown_fields");
    for n in [0u32, 10, 100] {
        let bytes = with_unknown(n);
        g.throughput(Throughput::Bytes(bytes.len() as u64));
        g.bench_with_input(BenchmarkId::new("preserve", n), &bytes, |b, by| {
            b.iter(|| Small::decode(black_box(by)).unwrap())
        });
        g.bench_with_input(BenchmarkId::new("raw_discard", n), &bytes, |b, by| {
            b.iter(|| {
                RawMessage::decode_filtered(
                    black_box(by),
                    &DecoderLimits::default(),
                    UnknownFieldPolicy::Discard,
                    &|id| id < 4,
                )
                .unwrap()
            })
        });
    }
    g.finish();
}

fn dynamic(c: &mut Criterion) {
    let compiled = tpt20_compiler::compile(tpt20_benches::SCHEMA, None).unwrap();
    let descriptor = tpt20_descriptor::Descriptor::new(compiled.ir);
    let message = descriptor.find_message("Nested").unwrap();
    let bytes = nested().encode();
    let limits = DecoderLimits::default();
    let mut g = c.benchmark_group("dynamic_decoding");
    g.bench_function("reflect/decode_and_get_field", |b| {
        b.iter(|| {
            let m = tpt20_reflect::DynamicMessage::decode(
                message,
                &descriptor,
                black_box(&bytes),
                &limits,
                UnknownFieldPolicy::Preserve,
            )
            .unwrap();
            black_box(m.get_field("tag").unwrap())
        })
    });
    g.bench_function("generated/decode", |b| {
        b.iter(|| Nested::decode(black_box(&bytes)).unwrap())
    });
    g.bench_function("raw/decode", |b| {
        b.iter(|| {
            RawMessage::decode(black_box(&bytes), &limits, UnknownFieldPolicy::Preserve).unwrap()
        })
    });
    g.finish();
}

fn json(c: &mut Criterion) {
    let mut g = c.benchmark_group("json");
    let s = small();
    let s_json = s.to_json().unwrap();
    g.bench_function("small/to_json", |b| {
        b.iter(|| black_box(&s).to_json().unwrap())
    });
    g.bench_function("small/from_json", |b| {
        b.iter(|| Small::from_json(black_box(&s_json)).unwrap())
    });
    let m = maps(100);
    let m_json = m.to_json().unwrap();
    g.throughput(Throughput::Bytes(m_json.len() as u64));
    g.bench_function("maps_100/to_json", |b| {
        b.iter(|| black_box(&m).to_json().unwrap())
    });
    g.bench_function("maps_100/from_json", |b| {
        b.iter(|| Maps::from_json(black_box(&m_json)).unwrap())
    });
    g.finish();
}

criterion_group!(
    benches,
    messages,
    packed,
    borrowed,
    unknown_fields,
    dynamic,
    json
);
criterion_main!(benches);
