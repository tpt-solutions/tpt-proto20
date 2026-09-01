use tpt20_compat_grpc::streaming;
use tpt20_rpc::stream::{TryStream, TrySink, BidiStream};

#[test]
fn streaming_module_exists() {
    let _ = streaming::StreamingSemantics;
}

#[test]
fn bidi_stream_type_exists() {
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
}

#[test]
fn stream_trait_has_correct_associated_types() {
    fn check_stream<T: TryStream<Item = Vec<u8>, Error = tpt20_rpc::ReceiveError> + Send + Sync>(_: &T) {}
    fn check_sink<T: TrySink<Item = Vec<u8>, Error = tpt20_rpc::SendError> + Send + Sync>(_: &T) {}
}
