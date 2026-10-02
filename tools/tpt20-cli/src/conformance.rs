//! `tpt20 conformance`: runs language-neutral JSON test vectors (spec §22)
//! against this implementation.
//!
//! A vector file is `{"suite": "...", "cases": [ ... ]}`; every case has a
//! `name` and a `kind`:
//!
//! - `decode` — `hex` bytes decode to `expect.fields` (or fail with
//!   `expect_error`, matched against the error variant name). Optional
//!   `limits` (`max_depth`, `max_message_bytes`, `max_field_count`,
//!   `max_string_bytes`), `policy` (`preserve`/`discard`/`fail`) and
//!   `known_ids`.
//! - `canonical` — the listed `fields` encode canonically to exactly `hex`,
//!   and decoding `hex` then canonically re-encoding gives `hex` again.
//! - `roundtrip` — decoding `hex` and re-encoding reproduces `hex`.
//! - `text` — with `schema` and `message`: `text` parses to the wire bytes
//!   `hex`, and `hex` prints back as exactly `text`.
//! - `schema` — `source` compiles (`expect: "ok"`) or is rejected with the
//!   diagnostic code in `expect_error_code`.
//!
//! Fields are `{"id": n, "class": "varint|fixed32|fixed64|len", "value": v}`
//! where `value` is a decimal string, or hex for `len`.

use serde_json::Value as Json;
use std::fs;
use std::path::Path;
use tpt20_core::{DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass};

/// Result of running one directory of vectors.
pub(crate) struct Summary {
    pub(crate) passed: usize,
    pub(crate) failed: usize,
}

/// Runs every `*.json` vector file in `dir` (optionally only files or cases
/// whose name equals `only`), printing one line per case.
pub(crate) fn run(dir: &Path, only: Option<&str>) -> Result<Summary, String> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    let mut summary = Summary {
        passed: 0,
        failed: 0,
    };
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let doc: Json = serde_json::from_str(&text)
            .map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?;
        let suite = doc.get("suite").and_then(Json::as_str).unwrap_or(stem);
        let cases = doc
            .get("cases")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("{}: missing `cases` array", path.display()))?;
        for case in cases {
            let name = case.get("name").and_then(Json::as_str).unwrap_or("?");
            if let Some(only) = only {
                if only != stem && only != suite && only != name {
                    continue;
                }
            }
            match run_case(case) {
                Ok(()) => {
                    println!("PASS {suite}/{name}");
                    summary.passed += 1;
                }
                Err(e) => {
                    eprintln!("FAIL {suite}/{name}: {e}");
                    summary.failed += 1;
                }
            }
        }
    }
    Ok(summary)
}

fn run_case(case: &Json) -> Result<(), String> {
    let kind = case
        .get("kind")
        .and_then(Json::as_str)
        .ok_or("case has no `kind`")?;
    match kind {
        "decode" => decode_case(case),
        "canonical" => canonical_case(case),
        "roundtrip" => roundtrip_case(case),
        "text" => text_case(case),
        "schema" => schema_case(case),
        other => Err(format!("unknown case kind `{other}`")),
    }
}

fn hex_field(case: &Json, key: &str) -> Result<Vec<u8>, String> {
    let s = case
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("missing `{key}`"))?;
    hex::decode(s.replace(' ', "")).map_err(|e| format!("`{key}` is not hex: {e}"))
}

fn limits(case: &Json) -> DecoderLimits {
    let mut l = DecoderLimits::default();
    if let Some(o) = case.get("limits").and_then(Json::as_object) {
        let get = |k: &str| o.get(k).and_then(Json::as_u64).map(|v| v as usize);
        if let Some(v) = get("max_depth") {
            l.max_depth = v;
        }
        if let Some(v) = get("max_message_bytes") {
            l.max_message_bytes = v;
        }
        if let Some(v) = get("max_field_count") {
            l.max_field_count = v;
        }
        if let Some(v) = get("max_string_bytes") {
            l.max_string_bytes = v;
        }
    }
    l
}

fn class_name(c: WireClass) -> &'static str {
    match c {
        WireClass::Varint => "varint",
        WireClass::Fixed32 => "fixed32",
        WireClass::Fixed64 => "fixed64",
        WireClass::Len => "len",
    }
}

fn field_to_json(f: &Field) -> Json {
    let value = match &f.value {
        Value::Varint(v) => v.to_string(),
        Value::Fixed32(v) => v.to_string(),
        Value::Fixed64(v) => v.to_string(),
        Value::Len(b) => hex::encode(b),
    };
    serde_json::json!({"id": f.field_id, "class": class_name(f.wire_class), "value": value})
}

fn parse_fields(list: &Json) -> Result<RawMessage, String> {
    let mut raw = RawMessage::new();
    for f in list.as_array().ok_or("`fields` must be an array")? {
        let id = f
            .get("id")
            .and_then(Json::as_u64)
            .ok_or("field without `id`")? as u32;
        let class = f
            .get("class")
            .and_then(Json::as_str)
            .ok_or("field without `class`")?;
        let value = f
            .get("value")
            .and_then(Json::as_str)
            .ok_or("field without `value`")?;
        let num = |v: &str| {
            v.parse::<u64>()
                .map_err(|e| format!("bad number `{v}`: {e}"))
        };
        let (wire, value) = match class {
            "varint" => (WireClass::Varint, Value::Varint(num(value)?)),
            "fixed32" => (WireClass::Fixed32, Value::Fixed32(num(value)? as u32)),
            "fixed64" => (WireClass::Fixed64, Value::Fixed64(num(value)?)),
            "len" => (
                WireClass::Len,
                Value::Len(hex::decode(value).map_err(|e| e.to_string())?),
            ),
            other => return Err(format!("unknown class `{other}`")),
        };
        raw.push(Field::new(id, wire, value));
    }
    Ok(raw)
}

fn decode_case(case: &Json) -> Result<(), String> {
    let bytes = hex_field(case, "hex")?;
    let policy = match case
        .get("policy")
        .and_then(Json::as_str)
        .unwrap_or("preserve")
    {
        "preserve" => UnknownFieldPolicy::Preserve,
        "discard" => UnknownFieldPolicy::Discard,
        "fail" => UnknownFieldPolicy::Fail,
        other => return Err(format!("unknown policy `{other}`")),
    };
    let known: Option<Vec<u32>> = case.get("known_ids").and_then(Json::as_array).map(|a| {
        a.iter()
            .filter_map(|v| v.as_u64().map(|n| n as u32))
            .collect()
    });
    let result = RawMessage::decode_filtered(&bytes, &limits(case), policy, &|id| {
        known.as_ref().map_or(true, |k| k.contains(&id))
    });
    match (result, case.get("expect_error").and_then(Json::as_str)) {
        (Ok(raw), None) => {
            let got: Vec<Json> = raw.fields.iter().map(field_to_json).collect();
            let want = case
                .get("expect")
                .and_then(|e| e.get("fields"))
                .and_then(Json::as_array)
                .ok_or("case needs `expect.fields` or `expect_error`")?;
            if &got == want {
                Ok(())
            } else {
                Err(format!(
                    "decoded {} but expected {}",
                    Json::Array(got),
                    Json::Array(want.clone())
                ))
            }
        }
        (Ok(_), Some(err)) => Err(format!("expected error `{err}` but decoding succeeded")),
        (Err(e), Some(want)) => {
            let got = format!("{e:?}");
            if got.starts_with(want) {
                Ok(())
            } else {
                Err(format!("expected error `{want}`, got `{got}`"))
            }
        }
        (Err(e), None) => Err(format!("unexpected decode error: {e:?}")),
    }
}

fn canonical_case(case: &Json) -> Result<(), String> {
    let want = hex_field(case, "hex")?;
    let raw = parse_fields(case.get("fields").ok_or("missing `fields`")?)?;
    let got = raw.encode_canonical().map_err(|e| e.to_string())?;
    if got != want {
        return Err(format!(
            "canonical bytes {} but expected {}",
            hex::encode(got),
            hex::encode(want)
        ));
    }
    let back = RawMessage::decode(
        &want,
        &DecoderLimits::default(),
        UnknownFieldPolicy::Preserve,
    )
    .map_err(|e| format!("canonical bytes do not decode: {e:?}"))?;
    let again = back.encode_canonical().map_err(|e| e.to_string())?;
    if again != want {
        return Err("canonical encoding is not idempotent".into());
    }
    Ok(())
}

fn roundtrip_case(case: &Json) -> Result<(), String> {
    let bytes = hex_field(case, "hex")?;
    let raw = RawMessage::decode(
        &bytes,
        &DecoderLimits::default(),
        UnknownFieldPolicy::Preserve,
    )
    .map_err(|e| format!("{e:?}"))?;
    let back = raw.encode().map_err(|e| e.to_string())?;
    if back == bytes {
        Ok(())
    } else {
        Err(format!(
            "re-encoded {} but expected {}",
            hex::encode(back),
            hex::encode(bytes)
        ))
    }
}

fn text_case(case: &Json) -> Result<(), String> {
    let schema = case
        .get("schema")
        .and_then(Json::as_str)
        .ok_or("missing `schema`")?;
    let message = case
        .get("message")
        .and_then(Json::as_str)
        .ok_or("missing `message`")?;
    let text = case
        .get("text")
        .and_then(Json::as_str)
        .ok_or("missing `text`")?;
    let want = hex_field(case, "hex")?;
    let compiled = tpt20_compiler::compile(schema, None).map_err(|d| {
        format!(
            "schema does not compile: {}",
            tpt20_compiler::render_all(&d)
        )
    })?;
    let descriptor = tpt20_descriptor::Descriptor::new(compiled.ir);
    let fmt = tpt20_text::TextFormat::new(&descriptor);
    let got = fmt
        .parse_to_bytes(message, text)
        .map_err(|e| e.to_string())?;
    if got != want {
        return Err(format!(
            "text parsed to {} but expected {}",
            hex::encode(got),
            hex::encode(&want)
        ));
    }
    let printed = fmt.print_bytes(message, &want).map_err(|e| e.to_string())?;
    if printed != text {
        return Err(format!(
            "bytes printed as {printed:?} but expected {text:?}"
        ));
    }
    Ok(())
}

fn schema_case(case: &Json) -> Result<(), String> {
    let source = case
        .get("source")
        .and_then(Json::as_str)
        .ok_or("missing `source`")?;
    let outcome = tpt20_compiler::compile(source, Some("vector.tpt"));
    match (
        outcome,
        case.get("expect_error_code").and_then(Json::as_str),
    ) {
        (Ok(_), None) => Ok(()),
        (Ok(_), Some(code)) => Err(format!(
            "expected diagnostic {code} but the schema compiled"
        )),
        (Err(diags), Some(code)) => {
            if diags.iter().any(|d| d.code == code) {
                Ok(())
            } else {
                let got: Vec<_> = diags.iter().map(|d| d.code.to_string()).collect();
                Err(format!("expected diagnostic {code}, got {got:?}"))
            }
        }
        (Err(diags), None) => Err(format!(
            "schema was rejected: {}",
            tpt20_compiler::render_all(&diags)
        )),
    }
}
