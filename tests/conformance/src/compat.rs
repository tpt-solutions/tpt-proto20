//! Compatibility conformance integration tests.

use tpt20_compat_protobuf::wire::{decode_protobuf, encode_protobuf};
use tpt20_compat_grpc::{decode_grpc_frame, encode_grpc_frame, from_grpc_status, to_grpc_status};
use tpt20_core::{Field, RawMessage, UnknownFieldPolicy, Value, WireClass};
use tpt20_rpc::Status;

#[test]
fn protobuf_schema_import() {
    let src = "syntax = \"proto3\"; message Foo { int32 x = 1; }";
    let tokens = tpt20_compat_protobuf::lex_proto(src).unwrap();
    let ast = tpt20_compat_protobuf::parse_proto(tokens).unwrap();
    let ir = tpt20_compat_protobuf::lower(ast).unwrap();
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].name, "Foo");
}

#[test]
fn protobuf_binary_roundtrip() {
    let mut msg = RawMessage::new();
    msg.push(Field::new(1, WireClass::Varint, Value::Varint(42)));
    msg.push(Field::new(2, WireClass::Len, Value::Len(b"hello".to_vec())));
    let bytes = encode_protobuf(&msg).unwrap();
    let back = decode_protobuf(&bytes).unwrap();
    assert_eq!(msg.fields, back.fields);
}

#[test]
fn grpc_frame_roundtrip() {
    let payload = b"hello world";
    let frame = encode_grpc_frame(payload, false).unwrap();
    let (flags, decoded) = decode_grpc_frame(&frame).unwrap();
    assert!(!flags.is_compressed());
    assert_eq!(decoded, payload);
}

#[test]
fn status_mapping_roundtrip() {
    for i in 0..=16 {
        let status = Status::from_code(i).unwrap();
        let grpc_code = to_grpc_status(status);
        assert_eq!(grpc_code, i);
        let back = from_grpc_status(grpc_code).unwrap();
        assert_eq!(back, status);
    }
}

#[test]
fn metadata_insert_enforces_rules() {
    use tpt20_rpc::Metadata;
    let mut md = Metadata::new(1024);
    assert!(md.insert_text("x-key", "value").is_ok());
    assert!(md.insert_binary("x-data-bin", b"hello").is_ok());
    assert!(md.insert_binary("x-data", b"hello").is_err());
}

#[test]
fn streaming_types_exist() {
    use tpt20_rpc::stream::{BidiStream, ClientStreamSource, ServerStreamSink, TrySink, TryStream};

    struct DummySink;
    impl TrySink for DummySink {
        type Item = Vec<u8>;
        type Error = tpt20_rpc::SendError;
        fn poll_ready(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> { std::task::Poll::Ready(Ok(())) }
        fn start_send(self: std::pin::Pin<&mut Self>, _item: Self::Item) -> Result<(), Self::Error> { Ok(()) }
        fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> { std::task::Poll::Ready(Ok(())) }
    }
    struct DummyStream;
    impl TryStream for DummyStream {
        type Item = Vec<u8>;
        type Error = tpt20_rpc::ReceiveError;
        fn poll_next(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Result<Self::Item, Self::Error>>> { std::task::Poll::Ready(None) }
    }

    let _bidi: BidiStream<Vec<u8>> = BidiStream::new(Box::new(DummySink), Box::new(DummyStream));
    let _sink: ServerStreamSink<Vec<u8>> = Box::new(DummySink);
    let _source: ClientStreamSource<Vec<u8>> = Box::new(DummyStream);
}
