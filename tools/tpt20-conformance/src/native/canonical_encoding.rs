use tpt20_core::{Field, RawMessage, Value, WireClass};

#[test]
fn canonical_roundtrip_independent_of_field_order() {
    let mut a = RawMessage::new();
    a.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
    a.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    let mut b = RawMessage::new();
    b.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    b.push(Field::new(2, WireClass::Varint, Value::Varint(1)));
    assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
}

#[test]
fn canonical_roundtrip_independent_of_unknown_field_order() {
    let mk = |id: u32| -> Vec<u8> {
        let mut m = RawMessage::new();
        m.push(Field::new(id, WireClass::Varint, Value::Varint(1)));
        m.encode_canonical().unwrap()
    };
    let ab = [mk(3), mk(1), mk(2)].concat();
    let ba = [mk(2), mk(3), mk(1)].concat();
    let dec = |b: &[u8]| {
        RawMessage::decode(b, &tpt20_core::DecoderLimits::default(), tpt20_core::UnknownFieldPolicy::Preserve).unwrap()
    };
    assert_eq!(
        dec(&ab).encode_canonical().unwrap(),
        dec(&ba).encode_canonical().unwrap()
    );
}

#[test]
fn canonical_oneof_last_wins() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(10, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(11, WireClass::Varint, Value::Varint(2)));
    msg.push(Field::new(10, WireClass::Varint, Value::Varint(3)));
    msg.canonical_reduce_oneofs(&[&[10, 11]]);
    let ids: Vec<u32> = msg.fields.iter().map(|f| f.field_id).collect();
    assert_eq!(ids, vec![10]);
    assert_eq!(msg.fields[0].value, Value::Varint(3));
}

#[test]
fn canonical_map_entry_ordering() {
    let entry = |k: &str, v: u64| -> Field {
        let mut e = RawMessage::new();
        e.push(Field::new(1, WireClass::Len, Value::Len(k.as_bytes().to_vec())));
        e.push(Field::new(2, WireClass::Varint, Value::Varint(v)));
        Field::new(5, WireClass::Len, Value::Len(e.encode().unwrap()))
    };
    let mut a = RawMessage::new();
    a.push(entry("zebra", 1));
    a.push(entry("apple", 2));
    a.push(Field::new(1, WireClass::Varint, Value::Varint(9)));
    let mut b = RawMessage::new();
    b.push(Field::new(1, WireClass::Varint, Value::Varint(9)));
    b.push(entry("apple", 2));
    b.push(entry("zebra", 1));

    a.canonical_sort_map_entries(&[5]);
    b.canonical_sort_map_entries(&[5]);
    assert_eq!(a.encode_canonical().unwrap(), b.encode_canonical().unwrap());
}

#[test]
fn canonical_reduce_oneofs_multiple_groups() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(1)));
    msg.push(Field::new(3, WireClass::Varint, Value::Varint(3)));
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(2)));
    msg.push(Field::new(5, WireClass::Varint, Value::Varint(5)));
    msg.push(Field::new(4, WireClass::Varint, Value::Varint(4)));
    msg.canonical_reduce_oneofs(&[&[1, 2], &[4, 5]]);
    let ids: Vec<u32> = msg.fields.iter().map(|f| f.field_id).collect();
    assert_eq!(ids, vec![1, 5]);
}

#[test]
fn canonical_is_deterministic_across_calls() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(99)));
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(99)));
    let a = msg.encode_canonical().unwrap();
    let b = msg.encode_canonical().unwrap();
    assert_eq!(a, b);
}
